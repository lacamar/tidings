use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, RandomState};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::time::Duration;

use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::{QString, QVariant};
use regex::Regex;
use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde_json::{Map, Value, json};
use zbus::zvariant::Value as DValue;

use crate::bridge::qobject::{App, FeedListModel, ItemListModel};
use crate::bus::{Bus, Setting, Signal};
use crate::db::{self, Fetch, KEEP_DAYS, Rule, Rules, now};
use crate::models::{Row, id, to_qvariant};
use crate::shim::{self, Fmt, fmt};
use crate::text::{capitalize, escape, plain, read_opml, summary, thin, write_opml};
use crate::web::{self, urljoin};

const REFRESH: Duration = Duration::from_secs(30 * 60);
const LIMIT: i64 = 300;

const STYLE: &str = r#"
:root{color-scheme:light;background:transparent}
:root.dark{color-scheme:dark}
body{font:17px/1.65 var(--font),system-ui,sans-serif;color:var(--fg);max-width:40em;margin:0 auto;padding:2.5em 1.75em 4em;overflow-wrap:break-word}
a{color:var(--accent);text-decoration-thickness:1px;text-underline-offset:.15em}
h1{font-size:1.75em;line-height:1.25;margin:0 0 .35em;letter-spacing:-.01em}
h1 a{color:inherit;text-decoration:none}
.meta{font-size:.85em;opacity:.6;margin-bottom:2.25em}
.source{margin-top:3em;padding-top:1em;border-top:1px solid rgba(127,127,127,.25);font-size:.85em;opacity:.7}
.loading{animation:pulse 1.2s ease-in-out infinite alternate}
@keyframes pulse{to{opacity:.25}}
img,video{max-width:100%;height:auto;border-radius:10px}
iframe{max-width:100%}
.video{width:100%;aspect-ratio:16/9;border:0;border-radius:10px;margin-bottom:1.5em}
audio{width:100%;margin-bottom:1.5em}
figure{margin:1.5em 0}
figcaption{font-size:.85em;opacity:.65;margin-top:.5em}
pre{overflow-x:auto;padding:1em 1.2em;border-radius:10px;background:rgba(127,127,127,.12)}
code{font:.88em "Adwaita Mono",monospace}
blockquote{margin:1.5em 0;padding-left:1.1em;border-left:3px solid var(--accent);opacity:.85}
hr{border:0;border-top:1px solid rgba(127,127,127,.25)}
table{border-collapse:collapse}
td,th{padding:.3em .6em;border:1px solid rgba(127,127,127,.25)}
"#;

static HAS_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</?[a-z][^>]*>").unwrap());
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://[^\s<]+").unwrap());
static QUOTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"["“](<q\b[^>]*>)|(</q>)["”]"#).unwrap());
static HTTP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^https?://\S+$").unwrap());

/// Taken by `start` once the UI is up.
pub static LISTENER: Mutex<Option<std::os::unix::net::UnixListener>> = Mutex::new(None);

fn xdg(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::home_dir().unwrap_or_default().join(default))
}

pub fn theme_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("tidings/theme.json")
}

fn wants() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
        .join("systemd/user/graphical-session.target.wants/tidings.service")
}

fn palette_base(dark: bool) -> Map<String, Value> {
    let v = if dark {
        json!({"bg": "#1c1c1f", "side": "#151517", "raised": "#2a2a2e", "fg": "#f2f2f4", "sub": "#9d9da6", "line": "#2b2b30"})
    } else {
        json!({"bg": "#ffffff", "side": "#f4f4f6", "raised": "#ffffff", "fg": "#1b1b20", "sub": "#6c6c76", "line": "#e6e6ea"})
    };
    let Value::Object(m) = v else { unreachable!() };
    m
}

fn when(ts: i64) -> String {
    let s = now() - ts;
    match s {
        86400.. => fmt(ts, Fmt::Time),
        3600.. => format!("{}h", s / 3600),
        60.. => format!("{}m", s / 60),
        _ => "now".into(),
    }
}

fn day(ts: i64) -> String {
    match shim::days_ago(ts) {
        ..=0 => "Today".into(),
        1 => "Yesterday".into(),
        n => capitalize(&fmt(ts, if n < 7 { Fmt::DayName } else { Fmt::LongDate })),
    }
}

fn base_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

type Job = Box<dyn FnOnce() + Send>;

struct Pool(mpsc::Sender<Job>);

impl Pool {
    fn new(n: usize) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..n {
            let rx = rx.clone();
            std::thread::spawn(move || {
                while let Ok(job) = rx.lock().unwrap().recv() {
                    job();
                }
            });
        }
        Self(tx)
    }
}

enum FileAction {
    Import,
    Export,
}

struct FeedRow {
    id: i64,
    url: String,
    etag: Option<String>,
    modified: Option<String>,
    icon: Option<String>,
    site: Option<String>,
    error: Option<String>,
    notify: bool,
    fetched: bool,
}

pub struct Backend {
    pub(crate) feed_model: *mut FeedListModel,
    pub(crate) item_model: *mut ItemListModel,
    db: Option<Connection>,
    rules: Rules,
    cache: PathBuf,
    pool: Option<Pool>,
    bus: Option<Bus>,
    feed: i64,
    current: i64,
    folder: String,
    folders: Map<String, Value>,
    unread: bool,
    query: String,
    dark: bool,
    accent: String,
    custom: Map<String, Value>,
    font: String,
    pending: Vec<(i64, Vec<i64>)>,
    note: u32,
    fresh: bool,
    requests: HashMap<String, FileAction>,
    inflight: HashSet<i64>,
    extracting: HashSet<i64>,
    iconing: HashSet<i64>,
    undo_ids: Vec<i64>,
    asked: i64,
    limit: i64,
    hidden: bool,
    calm: bool,
    expire: i64,
    hidden_n: i64,
    mutes: Vec<Value>,
    cosmetic: String,
    reload_gen: u64,
}

impl Default for Backend {
    fn default() -> Self {
        Self {
            feed_model: std::ptr::null_mut(),
            item_model: std::ptr::null_mut(),
            db: None,
            rules: Rules::default(),
            cache: PathBuf::new(),
            pool: None,
            bus: None,
            feed: 0,
            current: 0,
            folder: String::new(),
            folders: Map::new(),
            unread: false,
            query: String::new(),
            dark: false,
            accent: "#3584e4".into(),
            custom: Map::new(),
            font: String::new(),
            pending: Vec::new(),
            note: 0,
            fresh: false,
            requests: HashMap::new(),
            inflight: HashSet::new(),
            extracting: HashSet::new(),
            iconing: HashSet::new(),
            undo_ids: Vec::new(),
            asked: 0,
            limit: LIMIT,
            hidden: false,
            calm: false,
            expire: 0,
            hidden_n: 0,
            mutes: Vec::new(),
            cosmetic: String::new(),
            reload_gen: 0,
        }
    }
}

fn queue(qt: &CxxQtThread<App>, f: impl FnOnce(Pin<&mut App>) + Send + 'static) -> bool {
    qt.queue(f).is_ok()
}

fn every(qt: CxxQtThread<App>, period: Duration, f: fn(Pin<&mut App>)) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(period);
            if !queue(&qt, f) {
                return;
            }
        }
    });
}

impl App {
    fn r(self: Pin<&mut Self>) -> &mut Backend {
        self.rust_mut().get_mut()
    }

    fn db(&self) -> &Connection {
        self.db.as_ref().expect("start() opens the database")
    }

    fn feeds(&self) -> &FeedListModel {
        // SAFETY: set from C++ before start(), a child QObject that lives as long as self
        unsafe { &*self.feed_model }
    }

    fn items(&self) -> &ItemListModel {
        // SAFETY: as feeds()
        unsafe { &*self.item_model }
    }

    fn feeds_mut(self: Pin<&mut Self>) -> Pin<&mut FeedListModel> {
        // SAFETY: as feeds(); a separate QObject, so no alias of self
        unsafe { Pin::new_unchecked(&mut *self.feed_model) }
    }

    fn items_mut(self: Pin<&mut Self>) -> Pin<&mut ItemListModel> {
        // SAFETY: as feeds_mut()
        unsafe { Pin::new_unchecked(&mut *self.item_model) }
    }

    fn submit<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(Pin<&mut App>, T) + Send + 'static,
    ) {
        let qt = self.qt_thread();
        if let Some(p) = &self.pool {
            let _ = p.0.send(Box::new(move || {
                let r = job();
                queue(&qt, move |app| done(app, r));
            }));
        }
    }

    fn say(self: Pin<&mut Self>, text: &str) {
        self.message(QString::from(text), false);
    }

    pub fn start(mut self: Pin<&mut Self>, url: QString) {
        let data = xdg("XDG_DATA_HOME", ".local/share").join("tidings");
        let cache = xdg("XDG_CACHE_HOME", ".cache").join("tidings/icons");
        for d in [&data, &cache] {
            if let Err(e) = std::fs::create_dir_all(d) {
                eprintln!("{}: {e}", d.display());
            }
        }
        let db = match db::open_db(
            &data.join("tidings.db").to_string_lossy(),
            self.rules.clone(),
        ) {
            Ok(db) => db,
            Err(e) => {
                eprintln!("database: {e}");
                std::process::exit(1);
            }
        };
        let lists = cache.parent().map(Path::to_path_buf).unwrap_or_default();
        let r = self.as_mut().r();
        r.db = Some(db);
        r.cache = cache;
        r.pool = Some(Pool::new(8));
        r.font = format!("\"{}\"", shim::ui_font());
        self.as_mut().load_mutes(true);
        self.submit(
            move || crate::blocker::load(&lists),
            |mut app, js| {
                app.as_mut().r().cosmetic = js;
                app.cosmetic_changed();
            },
        );
        let qt = self.qt_thread();
        match Bus::new() {
            Ok(bus) => {
                let q = qt.clone();
                if let Err(e) = bus.listen(move |s| {
                    queue(&q, move |app| app.on_dbus(s));
                }) {
                    eprintln!("dbus signals: {e}");
                }
                for key in ["color-scheme", "accent-color"] {
                    match bus.read_setting(key) {
                        Ok(s) => self.as_mut().setting(s),
                        Err(e) => eprintln!("portal {key}: {e}"),
                    }
                }
                self.as_mut().r().bus = Some(bus);
            }
            Err(e) => eprintln!("dbus: {e}"),
        }
        self.as_mut().load_feeds();
        self.as_mut().load_items(true);
        every(qt.clone(), REFRESH, |app| app.refresh());
        every(qt.clone(), Duration::from_secs(60), |app| {
            app.load_items(false)
        });
        if let Some(listener) = LISTENER.lock().unwrap().take() {
            let q = qt.clone();
            std::thread::spawn(move || {
                for mut conn in listener.incoming().flatten() {
                    let _ = conn.set_read_timeout(Some(Duration::from_millis(500)));
                    let mut line = String::new();
                    let _ = BufReader::new(&mut conn).read_line(&mut line);
                    let u = line.trim().to_owned();
                    queue(&q, move |app| {
                        if u.is_empty() {
                            app.shown()
                        } else {
                            app.subscribe(QString::from(&u))
                        }
                    });
                }
            });
        }
        let url = url.to_string();
        if !url.is_empty() {
            queue(&qt, move |app| app.subscribe(QString::from(&url)));
        }
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(1));
            queue(&qt, |app| app.refresh());
        });
    }

    fn setting(mut self: Pin<&mut Self>, s: Setting) {
        match s {
            Setting::Dark(dark) => {
                self.as_mut().r().dark = dark;
                shim::set_color_scheme(dark);
            }
            Setting::Accent(r, g, b) if [r, g, b].iter().all(|c| (0.0..=1.0).contains(c)) => {
                let c = |v: f64| (v * 255.0).round() as u8;
                self.as_mut().r().accent = format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b));
            }
            _ => {}
        }
        self.theme_changed();
    }

    pub fn load_theme(mut self: Pin<&mut Self>) {
        let custom = std::fs::read(theme_path())
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| {
                if let Value::Object(m) = v {
                    Some(m)
                } else {
                    None
                }
            })
            .unwrap_or_default();
        self.as_mut().r().custom = custom;
        self.theme_changed();
    }

    fn palette(&self) -> Map<String, Value> {
        let mut p = palette_base(self.dark);
        p.insert("accent".into(), json!(self.accent));
        p.insert("opacity".into(), json!(0.7));
        p.extend(self.custom.clone());
        p
    }

    fn load_mutes(mut self: Pin<&mut Self>, force: bool) -> bool {
        let db = self.db();
        let gone = db
            .execute("delete from mutes where expires < ?", [now()])
            .unwrap_or(0);
        if gone == 0 && !force {
            return false;
        }
        type MuteRow = (i64, String, Option<i64>, bool, Option<i64>, Option<String>);
        let rows: Vec<MuteRow> = db
            .prepare(
                "select m.id, m.text, m.feed_id, m.title_only, m.expires, coalesce(f.name, f.title, f.url)
                 from mutes m left join feeds f on f.id = m.feed_id order by m.id",
            )
            .and_then(|mut s| {
                s.query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        r.get(2)?,
                        r.get::<_, Option<bool>>(3)?.unwrap_or(false),
                        r.get(4)?,
                        r.get(5)?,
                    ))
                })?
                .collect()
            })
            .unwrap_or_default();
        let mutes = rows
            .iter()
            .map(|(id, text, _, title_only, expires, feed)| {
                json!({
                    "id": id, "text": text, "feed": feed.clone().unwrap_or_default(), "titleOnly": title_only,
                    "expires": expires.filter(|e| *e != 0).map(|e| fmt(e, Fmt::ShortDate)).unwrap_or_default(),
                })
            })
            .collect();
        let rules = rows
            .iter()
            .filter_map(|(_, text, feed, title_only, ..)| {
                Some(Rule {
                    feed: *feed,
                    title_only: *title_only,
                    rx: db::mute_rule(text)?,
                })
            })
            .collect();
        *self.rules.write().unwrap() = rules;
        if let Err(e) = self.db().execute(
            "update items set muted = muted(feed_id, title, content)",
            [],
        ) {
            eprintln!("mutes: {e}");
        }
        self.as_mut().r().mutes = mutes;
        self.mutes_changed();
        true
    }

    pub fn add_mute(self: Pin<&mut Self>, text: QString, fid: i32, title_only: bool, days: i32) {
        let text = text.to_string();
        if text.trim_matches([' ', ',']).is_empty() {
            return;
        }
        let expires = (days != 0).then(|| now() + i64::from(days) * 86400);
        let _ = self.db().execute(
            "insert into mutes(text, feed_id, title_only, expires) values(?, ?, ?, ?)",
            params![text.trim(), (fid > 0).then_some(fid), title_only, expires],
        );
        self.refilter();
    }

    pub fn remove_mute(self: Pin<&mut Self>, mid: i32) {
        let _ = self.db().execute("delete from mutes where id = ?", [mid]);
        self.refilter();
    }

    fn refilter(mut self: Pin<&mut Self>) {
        self.as_mut().load_mutes(true);
        self.as_mut().load_feeds();
        self.load_items(false);
    }

    fn expire_old(&self) {
        if self.expire != 0 {
            let _ = self.db().execute(
                "update items set read = 1 where not read and not starred and published < ?",
                [now() - self.expire * 86400],
            );
        }
    }

    pub fn on_dbus(mut self: Pin<&mut Self>, s: Signal) {
        let (id, key) = match s {
            Signal::Setting(s) => return self.setting(s),
            Signal::Response { token, code, uri } => {
                let action = self.as_mut().r().requests.remove(&token);
                let path = uri.and_then(|u| url::Url::parse(&u).ok()?.to_file_path().ok());
                if let (Some(action), 0, Some(path)) = (action, code, path) {
                    match action {
                        FileAction::Import => self.import_path(&path),
                        FileAction::Export => self.save_opml(&path),
                    }
                }
                return;
            }
            Signal::Action(id, key) => (id, Some(key)),
            Signal::Closed(id) => (id, None),
            Signal::Token(id, token) => {
                if self.note != 0 && id == self.note {
                    // SAFETY: Qt reads this on the GUI thread when the window next activates; workers never touch it
                    unsafe { std::env::set_var("XDG_ACTIVATION_TOKEN", token) };
                }
                return;
            }
        };
        if self.note == 0 || id != self.note {
            return;
        }
        let r = self.as_mut().r();
        let pending = std::mem::take(&mut r.pending);
        r.note = 0;
        let ids: Vec<i64> = pending
            .iter()
            .flat_map(|(_, v)| v.iter().copied())
            .collect();
        let Some(key) = key else { return };
        if key == "read" {
            self.mark_ids(&ids, true);
            self.as_mut().load_feeds();
            self.load_items(false);
        } else {
            let fid = if pending.len() == 1 { pending[0].0 } else { 0 };
            let iid = if ids.len() == 1 { ids[0] } else { 0 };
            self.activate(fid as i32, iid as i32);
        }
    }

    fn mark_ids(&self, ids: &[i64], read: bool) {
        let db = self.db();
        let run = || -> rusqlite::Result<()> {
            let tx = db.unchecked_transaction()?;
            for i in ids {
                tx.execute("update items set read = ? where id = ?", params![read, i])?;
            }
            tx.commit()
        };
        if let Err(e) = run() {
            eprintln!("mark read: {e}");
        }
    }

    pub fn get_dark(&self) -> bool {
        self.dark
    }

    pub fn get_theme(&self) -> QVariant {
        to_qvariant(&Value::Object(self.palette()))
    }

    pub fn get_busy(&self) -> bool {
        !self.inflight.is_empty()
    }

    fn set_feed(mut self: Pin<&mut Self>, fid: i64, folder: &str) {
        let fid = if fid > 0 && self.feeds().find(fid).is_none() {
            0
        } else {
            fid
        };
        let folder = if fid == 0 && self.folders.contains_key(folder) {
            folder
        } else {
            ""
        };
        if (fid, folder) != (self.feed, self.folder.as_str()) {
            let r = self.as_mut().r();
            r.feed = fid;
            r.folder = folder.to_owned();
            self.as_mut().load_items(true);
            self.as_mut().changed();
        }
        let folder = folder.to_owned();
        self.withdraw(fid, &folder);
    }

    pub fn get_feed_id(&self) -> i32 {
        self.feed as i32
    }

    pub fn set_feed_id(self: Pin<&mut Self>, fid: i32) {
        self.set_feed(fid.into(), "");
    }

    pub fn get_unread_only(&self) -> bool {
        self.unread
    }

    pub fn set_unread_only(mut self: Pin<&mut Self>, on: bool) {
        self.as_mut().r().unread = on;
        self.as_mut().load_items(false);
        self.changed();
    }

    pub fn get_query(&self) -> QString {
        QString::from(&self.query)
    }

    pub fn set_query(mut self: Pin<&mut Self>, q: QString) {
        let q = q.to_string();
        if q != self.query {
            self.as_mut().r().query = q;
            self.as_mut().load_items(true);
            self.changed();
        }
    }

    pub fn get_folder(&self) -> QString {
        QString::from(&self.folder)
    }

    pub fn set_folder_scope(self: Pin<&mut Self>, folder: QString) {
        self.set_feed(0, &folder.to_string());
    }

    pub fn get_folders(&self) -> QVariant {
        to_qvariant(&Value::Object(self.folders.clone()))
    }

    pub fn get_show_hidden(&self) -> bool {
        self.hidden
    }

    pub fn set_show_hidden(mut self: Pin<&mut Self>, on: bool) {
        self.as_mut().r().hidden = on;
        self.as_mut().load_items(false);
        self.changed();
    }

    pub fn get_hidden_count(&self) -> i32 {
        self.hidden_n as i32
    }

    pub fn get_calm(&self) -> bool {
        self.calm
    }

    pub fn set_calm(mut self: Pin<&mut Self>, on: bool) {
        self.as_mut().r().calm = on;
        self.changed();
    }

    pub fn get_expire_days(&self) -> i32 {
        self.expire as i32
    }

    pub fn set_expire_days(mut self: Pin<&mut Self>, days: i32) {
        self.as_mut().r().expire = days.into();
        self.expire_old();
        self.as_mut().load_feeds();
        self.load_items(false);
    }

    pub fn get_mutes(&self) -> QVariant {
        to_qvariant(&Value::Array(self.mutes.clone()))
    }

    pub fn get_feed_title(&self) -> QString {
        if !self.folder.is_empty() {
            return QString::from(&self.folder);
        }
        let title = self
            .feeds()
            .rows
            .iter()
            .find(|r| id(r) == self.feed)
            .and_then(|r| r["title"].as_str());
        QString::from(title.unwrap_or(""))
    }

    pub fn get_cosmetic(&self) -> QString {
        QString::from(&self.cosmetic)
    }

    pub fn get_autostart(&self) -> bool {
        wants().symlink_metadata().is_ok()
    }

    pub fn set_autostart(self: Pin<&mut Self>, on: bool) {
        let verb = if on { "enable" } else { "disable" };
        if let Err(e) = std::process::Command::new("systemctl")
            .args(["--user", verb, "tidings.service"])
            .status()
        {
            eprintln!("systemctl: {e}");
        }
        self.changed();
    }

    pub fn tray_tip(&self) -> QString {
        if self.calm {
            return QString::from("Tidings");
        }
        let unread = self
            .feeds()
            .rows
            .first()
            .and_then(|r| r["unread"].as_i64())
            .unwrap_or(0);
        QString::from(&format!("Tidings — {unread} unread"))
    }

    fn scope(fid: i64, folder: &str) -> (String, Vec<Sql>) {
        if !folder.is_empty() {
            return (
                "feed_id in (select id from feeds where folder = ?)".into(),
                vec![Sql::Text(folder.into())],
            );
        }
        match fid {
            -1 => ("starred".into(), vec![]),
            0 => ("1".into(), vec![]),
            f => ("feed_id = ?".into(), vec![Sql::Integer(f)]),
        }
    }

    fn load_feeds(mut self: Pin<&mut Self>) {
        let db = self.db();
        let counts: HashMap<i64, i64> = db
            .prepare("select feed_id, count(*) from items where not read and dup is null and not muted group by feed_id")
            .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
            .unwrap_or_default();
        let saved: i64 = db
            .query_row("select count(*) from items where starred", [], |r| r.get(0))
            .unwrap_or(0);
        let blank = |id: i64, title: &str, unread: i64| {
            json!({"id": id, "title": title, "unread": unread, "web": 0, "icon": "", "error": "", "notify": 1,
                   "url": "", "site": "", "folder": "", "full": 0})
        };
        let mut rows = vec![
            blank(0, "All Articles", counts.values().sum()),
            blank(-1, "Saved", saved),
        ];
        let feeds = db
            .prepare(
                "select id, coalesce(name, title, url), web, icon, error, notify, url, site, folder, fulltext from feeds
                 order by coalesce(folder, '') collate nocase, coalesce(name, title, url) collate nocase",
            )
            .and_then(|mut s| {
                s.query_map([], |r| {
                    let id: i64 = r.get(0)?;
                    let text = |i| r.get::<_, Option<String>>(i).map(Option::unwrap_or_default);
                    Ok(json!({
                        "id": id, "title": text(1)?, "unread": counts.get(&id).copied().unwrap_or(0),
                        "web": r.get::<_, Option<i64>>(2)?, "icon": text(3)?, "error": text(4)?,
                        "notify": r.get::<_, Option<i64>>(5)?, "url": text(6)?, "site": text(7)?,
                        "folder": text(8)?, "full": r.get::<_, Option<i64>>(9)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap_or_default();
        rows.extend(feeds);
        let rows: Vec<Row> = rows
            .into_iter()
            .filter_map(|v| {
                if let Value::Object(m) = v {
                    Some(m)
                } else {
                    None
                }
            })
            .collect();
        let mut folders = Map::new();
        for r in &rows {
            if let Some(f) = r["folder"].as_str().filter(|f| !f.is_empty()) {
                let n = folders.get(f).and_then(Value::as_i64).unwrap_or(0)
                    + r["unread"].as_i64().unwrap_or(0);
                folders.insert(f.to_owned(), json!(n));
            }
        }
        self.as_mut().feeds_mut().set(rows, false);
        self.as_mut().r().folders = folders;
        if !self.folder.is_empty() && !self.folders.contains_key(&self.folder) {
            let feed = self.feed;
            self.as_mut().set_feed(feed, "");
        }
        self.changed();
    }

    fn load_items(mut self: Pin<&mut Self>, reset: bool) {
        if reset {
            self.as_mut().r().limit = LIMIT;
        }
        let (mut w, mut args) = Self::scope(self.feed, &self.folder);
        if self.unread {
            w += " and (not read or i.id = ?)";
            args.push(Sql::Integer(self.current));
        }
        if !self.query.is_empty() {
            w += " and (i.title like ? or i.content like ? or i.full like ?)";
            args.extend(std::iter::repeat_n(
                Sql::Text(format!("%{}%", self.query)),
                3,
            ));
        }
        let hide = "(i.dup is not null or i.muted)";
        let db = self.db();
        let n: i64 = if self.feed == -1 {
            0
        } else {
            db.query_row(
                &format!("select count(*) from items i where {w} and {hide}"),
                params_from_iter(&args),
                |r| r.get(0),
            )
            .unwrap_or(0)
        };
        if self.feed != -1 && !self.hidden {
            w += &format!(" and not {hide}");
        }
        args.push(Sql::Integer(self.limit));
        let q = format!(
            "select i.id, i.feed_id, i.title, i.link, i.published, i.image, i.read, i.starred, f.web, i.media_type,
             {hide} as hidden, coalesce(f.name, f.title) as feed_title,
             substr(coalesce(nullif(i.full, ''), i.content), 1, 3000) as body
             from items i join feeds f on f.id = i.feed_id where {w} order by published desc, i.id desc limit ?"
        );
        let rows: Vec<Row> = db
            .prepare(&q)
            .and_then(|mut s| {
                s.query_map(params_from_iter(&args), |r| {
                    let text = |k: &str| r.get::<_, Option<String>>(k).map(Option::unwrap_or_default);
                    let published: i64 = r.get::<_, Option<i64>>("published")?.unwrap_or(0);
                    let Value::Object(m) = json!({
                        "id": r.get::<_, i64>("id")?, "feed_id": r.get::<_, Option<i64>>("feed_id")?,
                        "title": plain(&text("title")?), "feed": text("feed_title")?,
                        "preview": plain(&text("body")?).chars().take(300).collect::<String>(),
                        "image": text("image")?, "when": when(published), "day": day(published),
                        "read": r.get::<_, Option<i64>>("read")?, "starred": r.get::<_, Option<i64>>("starred")?,
                        "link": text("link")?, "web": r.get::<_, Option<i64>>("web")?, "media": text("media_type")?,
                        "hidden": r.get::<_, Option<bool>>("hidden")?.unwrap_or(false),
                    }) else {
                        unreachable!()
                    };
                    Ok(m)
                })?
                .collect()
            })
            .unwrap_or_else(|e| {
                eprintln!("items: {e}");
                Vec::new()
            });
        self.as_mut().items_mut().set(rows, reset);
        if n != self.hidden_n {
            self.as_mut().r().hidden_n = n;
            self.changed();
        }
    }

    fn reload_soon(mut self: Pin<&mut Self>) {
        let r = self.as_mut().r();
        r.reload_gen += 1;
        let generation = r.reload_gen;
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            queue(&qt, move |app| {
                if app.reload_gen == generation {
                    app.load_items(false);
                }
            });
        });
    }

    fn patch(mut self: Pin<&mut Self>, iid: i64, col: &str, v: bool) {
        let _ = self.db().execute(
            &format!("update items set {col} = ? where id = ?"),
            params![v, iid],
        );
        if col == "read" && v {
            self.as_mut().forget(&[iid]);
        }
        if let Some(i) = self.items().find(iid) {
            let mut items = self.as_mut().items_mut();
            items.as_mut().rust_mut().rows[i].insert(col.into(), json!(i64::from(v)));
            items.changed_at(i);
        }
        self.as_mut().load_feeds();
        self.reload_soon();
    }

    pub fn more(mut self: Pin<&mut Self>) {
        if self.items().rows.len() as i64 >= self.limit {
            self.as_mut().r().limit += LIMIT;
            self.load_items(false);
        }
    }

    pub fn set_pos(self: Pin<&mut Self>, iid: i32, t: f64) {
        let _ = self
            .db()
            .execute("update items set pos = ? where id = ?", params![t, iid]);
    }

    fn row_at(&self, i: usize) -> QVariant {
        let mut r = self.items().rows[i].clone();
        r.insert("index".into(), json!(i));
        to_qvariant(&Value::Object(r))
    }

    pub fn neighbor(&self, iid: i32, step: i32) -> QVariant {
        let len = self.items().rows.len() as i64;
        let i = self
            .items()
            .find(iid.into())
            .map_or(if step > 0 { -1 } else { len }, |i| i as i64);
        let j = i + i64::from(step);
        if (0..len).contains(&j) {
            self.row_at(j as usize)
        } else {
            QVariant::default()
        }
    }

    pub fn feed_step(&self, n: i32) -> i32 {
        let ids: Vec<i64> = self.feeds().rows.iter().map(id).collect();
        let base = if self.folder.is_empty() {
            ids.iter().position(|&i| i == self.feed).unwrap_or(0)
        } else {
            0
        };
        ids[(base as i64 + i64::from(n)).rem_euclid(ids.len() as i64) as usize] as i32
    }

    pub fn next_unread(&self, iid: i32) -> QVariant {
        let rows = &self.items().rows;
        let i = self.items().find(iid.into());
        let after = i.map_or(0, |i| i + 1);
        let before = i.unwrap_or(0);
        (after..rows.len())
            .chain(0..before)
            .find(|&j| rows[j]["read"].as_i64().unwrap_or(0) == 0)
            .map_or_else(QVariant::default, |j| self.row_at(j))
    }

    pub fn next_feed(&self) -> i32 {
        let rows: Vec<&Row> = self.feeds().rows.iter().filter(|r| id(r) > 0).collect();
        let i = rows.iter().position(|r| id(r) == self.feed);
        let after = i.map_or(0, |i| i + 1);
        // Python's rows[:-1] when the current feed is not a real one
        let before = i.unwrap_or(rows.len().saturating_sub(1));
        rows[after..]
            .iter()
            .chain(&rows[..before])
            .find(|r| r["unread"].as_i64().unwrap_or(0) != 0)
            .map_or(0, |r| id(r) as i32)
    }

    pub fn article(mut self: Pin<&mut Self>, iid: i32) -> QString {
        let iid = i64::from(iid);
        self.as_mut().r().current = iid;
        struct Art {
            read: bool,
            content: String,
            full: Option<String>,
            link: Option<String>,
            media_type: Option<String>,
            media: Option<String>,
            fulltext: bool,
            feed_title: Option<String>,
            author: Option<String>,
            published: i64,
            pos: Option<f64>,
            title: String,
        }
        let a = self.db().query_row(
            "select i.read, i.content, i.full, i.link, i.media_type, i.media, f.fulltext, coalesce(f.name, f.title),
                    i.author, i.published, i.pos, i.title
             from items i join feeds f on f.id = i.feed_id where i.id = ?",
            [iid],
            |r| {
                Ok(Art {
                    read: r.get::<_, Option<bool>>(0)?.unwrap_or(false),
                    content: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    full: r.get(2)?,
                    link: r.get::<_, Option<String>>(3)?.filter(|l| !l.is_empty()),
                    media_type: r.get(4)?,
                    media: r.get(5)?,
                    fulltext: r.get::<_, Option<bool>>(6)?.unwrap_or(false),
                    feed_title: r.get(7)?,
                    author: r.get(8)?,
                    published: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
                    pos: r.get(10)?,
                    title: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
                })
            },
        );
        let Ok(a) = a else { return QString::default() };
        if !a.read {
            self.as_mut().patch(iid, "read", true);
        }
        let (mut body, mut extra) = (a.content.clone(), String::new());
        match (&a.full, &a.link) {
            (Some(full), _) if !full.is_empty() => {
                body = full.clone();
                if !plain(&a.content).is_empty() {
                    extra = format!("<div class=\"source\">{}</div>", a.content);
                }
            }
            (None, Some(link))
                if a.media_type.as_deref() != Some("youtube")
                    && (a.fulltext || thin(&a.content)) =>
            {
                self.as_mut().fetch_full(iid, link.clone());
            }
            _ => {}
        }
        if self.extracting.contains(&iid) {
            extra = "<p class=\"meta loading\">Fetching full article…</p>".into();
        }
        if !HAS_TAG.is_match(&body) {
            body = format!(
                "<p style=\"white-space:pre-line\">{}</p>",
                URL.replace_all(&escape(&body), "<a href=\"$0\">$0</a>")
            );
        }
        let body = QUOTE.replace_all(&body, "${1}${2}");
        let mins = (plain(&body).split(' ').filter(|w| !w.is_empty()).count() as f64 / 230.0)
            .round_ties_even() as i64;
        let mut meta: Vec<String> = Vec::new();
        let read_time = (mins != 0 && a.media.as_deref().is_none_or(str::is_empty))
            .then(|| format!("{mins} min read"));
        for part in [
            a.feed_title,
            a.author,
            Some(fmt(a.published, Fmt::LongDate)),
            read_time,
        ]
        .into_iter()
        .flatten()
        {
            if !part.is_empty() && !meta.contains(&part) {
                meta.push(part);
            }
        }
        let t = a.media_type.unwrap_or_default();
        let mut src = escape(a.media.as_deref().unwrap_or(""));
        if let Some(pos) = a.pos.filter(|p| *p != 0.0)
            && t != "youtube"
        {
            src += &format!("#t={}", pos.trunc() as i64);
        }
        let player = match t.as_str() {
            "youtube" => format!("<iframe class=\"video\" src=\"{src}\" allowfullscreen></iframe>"),
            "" => String::new(),
            t => format!("<{t} controls preload=\"metadata\" src=\"{src}\"></{t}>"),
        };
        let p = self.palette();
        let s = |k: &str| {
            p.get(k)
                .map(|v| v.as_str().map_or_else(|| v.to_string(), String::from))
                .unwrap_or_default()
        };
        QString::from(&format!(
            "<!doctype html><html class=\"{}\" style=\"--accent:{};--fg:{};--font:{}\"><meta charset=\"utf-8\">\
             <script>history.scrollRestoration=\"manual\"</script><style>{STYLE}</style>\
             <h1><a href=\"{}\">{}</a></h1><div class=\"meta\">{}</div>{player}{body}{extra}",
            if self.dark { "dark" } else { "" },
            s("accent"),
            s("fg"),
            escape(&self.font),
            escape(a.link.as_deref().unwrap_or("")),
            escape(&plain(&a.title)),
            escape(&meta.join(" · ")),
        ))
    }

    fn fetch_full(mut self: Pin<&mut Self>, iid: i64, link: String) {
        if self.as_mut().r().extracting.insert(iid) {
            self.submit(
                move || web::extract(&link),
                move |app, res| app.extracted(iid, res),
            );
        }
    }

    pub fn toggle_full(mut self: Pin<&mut Self>, iid: i32) {
        let iid = i64::from(iid);
        let row: Option<(Option<String>, Option<String>)> = self
            .db()
            .query_row("select full, link from items where id = ?", [iid], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
            .ok()
            .flatten();
        let (full, link) = row.unwrap_or_default();
        if full.is_some_and(|f| !f.is_empty()) {
            let _ = self
                .db()
                .execute("update items set full = '' where id = ?", [iid]);
            self.as_mut().reload_soon();
        } else if let Some(link) = link.filter(|l| !l.is_empty()) {
            self.as_mut().r().asked = iid;
            self.as_mut().fetch_full(iid, link);
        }
        self.article_ready(iid as i32);
    }

    fn prefetch(mut self: Pin<&mut Self>) {
        let rows: Vec<(i64, String, String, bool)> = self
            .db()
            .prepare(
                "select i.id, link, content, fulltext from items i join feeds f on f.id = feed_id where not read
                 and full is null and link is not null and media_type is not 'youtube'
                 order by published desc limit 100",
            )
            .and_then(|mut s| {
                s.query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        r.get::<_, Option<bool>>(3)?.unwrap_or(false),
                    ))
                })?
                .collect()
            })
            .unwrap_or_default();
        for (id, link, _, _) in rows
            .into_iter()
            .filter(|(_, _, c, full)| *full || thin(c))
            .take(20)
        {
            self.as_mut().fetch_full(id, link);
        }
    }

    fn extracted(
        mut self: Pin<&mut Self>,
        iid: i64,
        res: Result<(String, Option<String>), String>,
    ) {
        self.as_mut().r().extracting.remove(&iid);
        let (body, og) = res.unwrap_or_else(|e| {
            eprintln!("extract {iid}: {e}");
            (String::new(), None)
        });
        if iid == self.asked {
            self.as_mut().r().asked = 0;
            if body.is_empty() {
                self.as_mut().say("Couldn't find the full article");
            }
        }
        let _ = self.db().execute(
            "update items set full = ?, image = coalesce(image, ?) where id = ?",
            params![body, og, iid],
        );
        self.as_mut().reload_soon();
        if iid == self.current {
            self.article_ready(iid as i32);
        }
    }

    fn fetch_icon(mut self: Pin<&mut Self>, fid: i64, site: String) {
        if site.is_empty() || !self.as_mut().r().iconing.insert(fid) {
            return;
        }
        let path = self
            .cache
            .join(format!("{fid}.png"))
            .to_string_lossy()
            .into_owned();
        self.submit(
            move || web::favicon(&site, |b| shim::save_icon(b, &path)).map(|()| path),
            move |mut app, res| match res {
                Ok(path) => {
                    let _ = app
                        .db()
                        .execute("update feeds set icon = ? where id = ?", params![path, fid]);
                    app.as_mut().load_feeds();
                }
                Err(e) => eprintln!("icon {fid}: {e}"),
            },
        );
    }

    pub fn set_read(self: Pin<&mut Self>, iid: i32, on: bool) {
        self.patch(iid.into(), "read", on);
    }

    pub fn set_starred(self: Pin<&mut Self>, iid: i32, on: bool) {
        self.patch(iid.into(), "starred", on);
    }

    fn set_feed_col(mut self: Pin<&mut Self>, fid: i32, col: &str, v: Sql) {
        let _ = self.db().execute(
            &format!("update feeds set {col} = ? where id = ?"),
            params![v, fid],
        );
        self.as_mut().load_feeds();
        self.load_items(false);
    }

    pub fn set_web(self: Pin<&mut Self>, fid: i32, on: bool) {
        self.set_feed_col(fid, "web", Sql::Integer(on.into()));
    }

    pub fn set_serif(mut self: Pin<&mut Self>, on: bool) {
        self.as_mut().r().font = if on {
            "serif".into()
        } else {
            format!("\"{}\"", shim::ui_font())
        };
        let current = self.current as i32;
        self.article_ready(current);
    }

    pub fn set_full(self: Pin<&mut Self>, fid: i32, on: bool) {
        self.set_feed_col(fid, "fulltext", Sql::Integer(on.into()));
    }

    pub fn set_notify(self: Pin<&mut Self>, fid: i32, on: bool) {
        self.set_feed_col(fid, "notify", Sql::Integer(on.into()));
    }

    fn text_or_null(s: &QString) -> Sql {
        let s = s.to_string();
        let s = s.trim();
        if s.is_empty() {
            Sql::Null
        } else {
            Sql::Text(s.into())
        }
    }

    pub fn rename_feed(self: Pin<&mut Self>, fid: i32, name: QString) {
        self.set_feed_col(fid, "name", Self::text_or_null(&name));
    }

    pub fn set_folder(self: Pin<&mut Self>, fid: i32, name: QString) {
        self.set_feed_col(fid, "folder", Self::text_or_null(&name));
    }

    pub fn rename_folder(mut self: Pin<&mut Self>, old: QString, to: QString) {
        let old = old.to_string();
        let _ = self.db().execute(
            "update feeds set folder = ? where folder = ?",
            params![Self::text_or_null(&to), old],
        );
        if self.folder == old {
            self.as_mut().r().folder = to.to_string().trim().to_owned();
        }
        self.as_mut().load_feeds();
        self.load_items(false);
    }

    pub fn refresh(mut self: Pin<&mut Self>) {
        let feeds: Vec<FeedRow> = self
            .db()
            .prepare(
                "select id, url, etag, modified, icon, site, error, notify, fetched from feeds",
            )
            .and_then(|mut s| {
                s.query_map([], |r| {
                    Ok(FeedRow {
                        id: r.get(0)?,
                        url: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        etag: r.get(2)?,
                        modified: r.get(3)?,
                        icon: r.get(4)?,
                        site: r.get(5)?,
                        error: r.get(6)?,
                        notify: r.get::<_, Option<bool>>(7)?.unwrap_or(false),
                        fetched: r.get::<_, Option<i64>>(8)?.is_some_and(|f| f != 0),
                    })
                })?
                .collect()
            })
            .unwrap_or_default();
        for f in feeds {
            if self.as_mut().r().inflight.insert(f.id) {
                let (url, etag, modified) = (f.url.clone(), f.etag.clone(), f.modified.clone());
                self.submit(
                    move || web::fetch_feed(&url, etag.as_deref(), modified.as_deref()),
                    move |app, res| app.fetched(f, res),
                );
            }
        }
        self.busy_changed();
    }

    fn fetched(mut self: Pin<&mut Self>, f: FeedRow, res: Result<Fetch, String>) {
        self.as_mut().r().inflight.remove(&f.id);
        let mut new = Vec::new();
        let err = match res {
            Ok(fetch) => match db::store(self.db(), f.id, &fetch) {
                Ok(n) => {
                    new = n;
                    if f.icon.as_deref().is_none_or(str::is_empty) {
                        let link = match &fetch {
                            Fetch::Feed { feed, .. } => db::link_of(&feed.links).map(String::from),
                            Fetch::NotModified => None,
                        };
                        let site = link
                            .or_else(|| f.site.clone().filter(|s| !s.is_empty()))
                            .unwrap_or_else(|| urljoin(&f.url, "/"));
                        self.as_mut().fetch_icon(f.id, site);
                    }
                    None
                }
                Err(e) => Some(e.to_string()),
            },
            Err(e) => Some(e),
        };
        if err != f.error {
            let _ = self.db().execute(
                "update feeds set error = ? where id = ?",
                params![err, f.id],
            );
        }
        if !new.is_empty() && f.notify && f.fetched && !shim::window_active() {
            let r = self.as_mut().r();
            match r.pending.iter_mut().find(|(fid, _)| *fid == f.id) {
                Some((_, ids)) => {
                    ids.splice(0..0, new);
                }
                None => r.pending.push((f.id, new)),
            }
            r.fresh = true;
        }
        if !self.inflight.is_empty() {
            return;
        }
        self.as_mut().load_mutes(false);
        self.expire_old();
        let _ = self.db().execute(
            "delete from items where read and not starred and published < ?",
            [now() - KEEP_DAYS * 86400],
        );
        self.as_mut().busy_changed();
        self.as_mut().prefetch();
        self.as_mut().load_feeds();
        self.as_mut().load_items(false);
        if self.fresh {
            self.as_mut().r().fresh = false;
            self.notify();
        }
    }

    fn notify(mut self: Pin<&mut Self>) {
        let ids: Vec<i64> = self
            .pending
            .iter()
            .flat_map(|(_, v)| v.iter().copied())
            .collect();
        let marks = vec!["?"; ids.len()].join(",");
        let rows: Vec<(String, String)> = self
            .db()
            .prepare(&format!(
                "select i.title, coalesce(f.name, f.title, f.url) from items i join feeds f on f.id = i.feed_id
                 where i.dup is null and not i.muted and i.id in ({marks}) order by i.published desc"
            ))
            .and_then(|mut s| {
                s.query_map(params_from_iter(&ids), |r| {
                    Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, Option<String>>(1)?.unwrap_or_default()))
                })?
                .collect()
            })
            .unwrap_or_default();
        if rows.is_empty() {
            return;
        }
        let (head, body) = summary(&rows);
        let Some(bus) = &self.bus else { return };
        match bus.notify(self.note, &head, &htmlize::escape_text(&body)) {
            Ok(id) => self.as_mut().r().note = id,
            Err(e) => eprintln!("notify: {e}"),
        }
    }

    fn withdraw(mut self: Pin<&mut Self>, fid: i64, folder: &str) {
        let gone: Vec<i64> = if !folder.is_empty() {
            self.feeds()
                .rows
                .iter()
                .filter(|r| r["folder"] == folder)
                .map(id)
                .collect()
        } else if fid == 0 {
            self.pending.iter().map(|(f, _)| *f).collect()
        } else {
            vec![fid]
        };
        let ids: Vec<i64> = self
            .pending
            .iter()
            .filter(|(f, _)| gone.contains(f))
            .flat_map(|(_, v)| v.iter().copied())
            .collect();
        self.as_mut().forget(&ids);
    }

    fn forget(mut self: Pin<&mut Self>, ids: &[i64]) {
        let r = self.as_mut().r();
        let count = |p: &[(i64, Vec<i64>)]| p.iter().map(|(_, v)| v.len()).sum::<usize>();
        let n = count(&r.pending);
        for (_, v) in &mut r.pending {
            v.retain(|i| !ids.contains(i));
        }
        r.pending.retain(|(_, v)| !v.is_empty());
        if r.note == 0 || n == count(&r.pending) {
            return;
        }
        if !r.pending.is_empty() {
            return self.notify();
        }
        if let Some(bus) = &r.bus {
            let _ = bus.close(r.note);
        }
        r.note = 0;
    }

    pub fn add_feed(self: Pin<&mut Self>, url: QString) {
        let url = url.to_string().trim().to_owned();
        self.submit(move || web::discover(&url), |app, res| app.discovered(res));
    }

    fn discovered(mut self: Pin<&mut Self>, res: Result<(String, Fetch), String>) {
        let (url, fetch) = match res {
            Ok(r) => r,
            Err(e) => return self.added(QString::from(&e)),
        };
        if self
            .db()
            .execute("insert into feeds(url) values(?)", [&url])
            .is_err()
        {
            return self.added(QString::from("You're already subscribed to that feed"));
        }
        let fid = self.db().last_insert_rowid();
        if let Err(e) = db::store(self.db(), fid, &fetch) {
            eprintln!("store {fid}: {e}");
        }
        self.as_mut().prefetch();
        let site = match &fetch {
            Fetch::Feed { feed, .. } => db::link_of(&feed.links).map(String::from),
            Fetch::NotModified => None,
        };
        self.as_mut()
            .fetch_icon(fid, site.unwrap_or_else(|| urljoin(&url, "/")));
        self.as_mut().added(QString::default());
        self.as_mut().load_feeds();
        self.set_feed(fid, "");
    }

    pub fn remove_feed(mut self: Pin<&mut Self>, fid: i32) {
        let fid = i64::from(fid);
        self.as_mut().withdraw(fid, "");
        let _ = self.db().execute("delete from feeds where id = ?", [fid]);
        self.as_mut().load_feeds();
        if self.feed == fid {
            self.set_feed(0, "")
        } else {
            self.load_items(false)
        }
    }

    pub fn mark_all_read(self: Pin<&mut Self>, fid: i32, folder: QString) {
        self.mark_older_read(fid, folder, 0);
    }

    pub fn mark_older_read(mut self: Pin<&mut Self>, fid: i32, folder: QString, older_than: i32) {
        let (mut w, mut args) = Self::scope(fid.into(), &folder.to_string());
        if older_than != 0 {
            w += " and published < (select published from items where id = ?)";
            args.push(Sql::Integer(older_than.into()));
        }
        let ids: Vec<i64> = self
            .db()
            .prepare(&format!("select id from items where not read and {w}"))
            .and_then(|mut s| {
                s.query_map(params_from_iter(&args), |r| r.get(0))?
                    .collect()
            })
            .unwrap_or_default();
        self.as_mut().r().undo_ids = ids.clone();
        if ids.is_empty() {
            return;
        }
        self.mark_ids(&ids, true);
        self.as_mut().forget(&ids);
        self.as_mut().load_feeds();
        self.as_mut().load_items(false);
        let n = ids.len();
        self.message(
            QString::from(&format!("Marked {n} article{} as read", plural(n))),
            true,
        );
    }

    pub fn undo(mut self: Pin<&mut Self>) {
        let ids = std::mem::take(&mut self.as_mut().r().undo_ids);
        self.mark_ids(&ids, false);
        self.as_mut().load_feeds();
        self.load_items(false);
    }

    pub fn clipboard_url(&self) -> QString {
        let t = shim::clipboard_text();
        let t = t.trim();
        QString::from(if HTTP.is_match(t) { t } else { "" })
    }

    pub fn copy(self: Pin<&mut Self>, text: QString) {
        self.copy_what(text, QString::from("Link"));
    }

    pub fn copy_what(self: Pin<&mut Self>, text: QString, what: QString) {
        shim::set_clipboard(&text.to_string());
        self.say(&format!("{what} copied"));
    }

    fn choose(
        mut self: Pin<&mut Self>,
        method: &str,
        title: &str,
        opts: HashMap<&str, DValue>,
        action: FileAction,
    ) {
        let token = format!("tidings{:08x}", RandomState::new().hash_one(now()) as u32);
        let res = match &self.bus {
            Some(bus) => bus
                .choose(method, title, &token, opts)
                .map_err(|e| e.to_string()),
            None => Err("no session bus".into()),
        };
        match res {
            Ok(()) => {
                self.as_mut().r().requests.insert(token, action);
            }
            Err(e) => self.say(&format!("No file chooser available ({e})")),
        }
    }

    pub fn import_opml(self: Pin<&mut Self>) {
        let filters = vec![("OPML", vec![(0u32, "*.opml"), (0u32, "*.xml")])];
        self.choose(
            "OpenFile",
            "Import Subscriptions",
            HashMap::from([("filters", DValue::from(filters))]),
            FileAction::Import,
        );
    }

    pub fn import_file(self: Pin<&mut Self>, path: QString) {
        self.import_path(Path::new(&path.to_string()));
    }

    fn import_path(mut self: Pin<&mut Self>, path: &Path) {
        let feeds = std::fs::read(path)
            .ok()
            .and_then(|b| read_opml(&String::from_utf8_lossy(&b)).ok());
        let Some(feeds) = feeds else {
            return self.say(&format!("Couldn't read {}", base_name(path)));
        };
        let n: usize = feeds
            .iter()
            .map(|(url, title, folder)| {
                self.db()
                    .execute(
                        "insert or ignore into feeds(url, title, folder) values(?, ?, ?)",
                        params![url, title, folder],
                    )
                    .unwrap_or(0)
            })
            .sum();
        let msg = if n == 0 {
            "Already subscribed to all of those".into()
        } else {
            format!("Added {n} feed{}", plural(n))
        };
        self.as_mut().say(&msg);
        self.as_mut().load_feeds();
        self.refresh();
    }

    pub fn export_opml(self: Pin<&mut Self>) {
        self.choose(
            "SaveFile",
            "Export Subscriptions",
            HashMap::from([("current_name", DValue::from("Subscriptions.opml"))]),
            FileAction::Export,
        );
    }

    fn save_opml(self: Pin<&mut Self>, path: &Path) {
        let feeds: Vec<(String, String, Option<String>, Option<String>)> = self
            .db()
            .prepare("select url, coalesce(name, title, url), site, folder from feeds order by coalesce(folder, '')")
            .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect())
            .unwrap_or_default();
        match std::fs::write(path, write_opml(&feeds)) {
            Ok(()) => self.say(&format!("Saved {}", base_name(path))),
            Err(e) => eprintln!("export {}: {e}", path.display()),
        }
    }
}
