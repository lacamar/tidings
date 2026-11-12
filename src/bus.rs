use std::collections::HashMap;
use std::time::Duration;

use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{MatchRule, Message};

const NOTES: (&str, &str, &str) = (
    "/org/freedesktop/Notifications",
    "org.freedesktop.Notifications",
    "org.freedesktop.Notifications",
);
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const FILES: &str = "org.freedesktop.portal.FileChooser";

pub enum Setting {
    Dark(bool),
    Accent(f64, f64, f64),
    Other,
}

pub enum Signal {
    Setting(Setting),
    Response {
        token: String,
        code: u32,
        uri: Option<String>,
    },
    Action(u32, String),
    Closed(u32),
    Token(u32, String),
}

fn inner<'a>(v: &'a Value<'a>) -> &'a Value<'a> {
    match v {
        Value::Value(v) => inner(v),
        v => v,
    }
}

fn setting(key: &str, v: &Value) -> Setting {
    match (key, inner(v)) {
        ("color-scheme", Value::U32(n)) => Setting::Dark(*n == 1),
        ("color-scheme", _) => Setting::Dark(false),
        ("accent-color", v) => <(f64, f64, f64)>::try_from(v.try_clone().unwrap_or(Value::U8(0)))
            .map_or(Setting::Other, |(r, g, b)| Setting::Accent(r, g, b)),
        _ => Setting::Other,
    }
}

pub struct Bus(Connection);

impl Bus {
    pub fn new() -> zbus::Result<Self> {
        Ok(Self(
            zbus::blocking::connection::Builder::session()?
                .method_timeout(Duration::from_secs(5))
                .build()?,
        ))
    }

    pub fn read_setting(&self, key: &str) -> zbus::Result<Setting> {
        let r = self.0.call_method(
            Some(PORTAL),
            PORTAL_PATH,
            Some(SETTINGS),
            "ReadOne",
            &("org.freedesktop.appearance", key),
        )?;
        let v: OwnedValue = r.body().deserialize()?;
        Ok(setting(key, &v))
    }

    pub fn notify(&self, replaces: u32, head: &str, body: &str) -> zbus::Result<u32> {
        let hints = HashMap::from([
            ("desktop-entry", Value::from(crate::APP_ID)),
            ("transient", Value::from(true)),
        ]);
        let r = self.0.call_method(
            Some(NOTES.1),
            NOTES.0,
            Some(NOTES.2),
            "Notify",
            &(
                "Tidings",
                replaces,
                "application-rss+xml",
                head,
                body,
                vec!["default", "Open", "read", "Mark as Read"],
                hints,
                -1i32,
            ),
        )?;
        r.body().deserialize()
    }

    pub fn close(&self, id: u32) -> zbus::Result<()> {
        self.0
            .call_method(
                Some(NOTES.1),
                NOTES.0,
                Some(NOTES.2),
                "CloseNotification",
                &(id,),
            )
            .map(drop)
    }

    pub fn choose<'a>(
        &self,
        method: &str,
        title: &str,
        token: &'a str,
        mut opts: HashMap<&str, Value<'a>>,
    ) -> zbus::Result<()> {
        opts.insert("handle_token", Value::from(token));
        self.0
            .call_method(
                Some(PORTAL),
                PORTAL_PATH,
                Some(FILES),
                method,
                &("", title, opts),
            )
            .map(drop)
    }

    /// Calls `f` from background threads for every signal the backend reacts to.
    pub fn listen(&self, f: impl Fn(Signal) + Send + Clone + 'static) -> zbus::Result<()> {
        let rules = [
            MatchRule::builder()
                .msg_type(Type::Signal)
                .interface(NOTES.2)?
                .build(),
            MatchRule::builder()
                .msg_type(Type::Signal)
                .interface(SETTINGS)?
                .member("SettingChanged")?
                .build(),
            MatchRule::builder()
                .msg_type(Type::Signal)
                .interface("org.freedesktop.portal.Request")?
                .member("Response")?
                .build(),
        ];
        for rule in rules {
            let it = MessageIterator::for_match_rule(rule, &self.0, None)?;
            let f = f.clone();
            std::thread::spawn(move || {
                for m in it.flatten() {
                    if let Some(s) = decode(&m) {
                        f(s);
                    }
                }
            });
        }
        Ok(())
    }
}

fn decode(m: &Message) -> Option<Signal> {
    let h = m.header();
    let b = m.body();
    Some(match h.member()?.as_str() {
        "SettingChanged" => {
            let (ns, key, v): (String, String, OwnedValue) = b.deserialize().ok()?;
            if ns != "org.freedesktop.appearance" {
                return None;
            }
            Signal::Setting(setting(&key, &v))
        }
        "Response" => {
            let (code, results): (u32, HashMap<String, OwnedValue>) = b.deserialize().ok()?;
            let uri = results
                .get("uris")
                .and_then(|v| Vec::<String>::try_from(v.try_clone().ok()?).ok())
                .and_then(|u| u.into_iter().next());
            let token = h.path()?.as_str().rsplit('/').next()?.to_owned();
            Signal::Response { token, code, uri }
        }
        "ActionInvoked" => {
            let (id, key): (u32, String) = b.deserialize().ok()?;
            Signal::Action(id, key)
        }
        "NotificationClosed" => Signal::Closed(b.deserialize::<(u32, u32)>().ok()?.0),
        "ActivationToken" => {
            let (id, token): (u32, String) = b.deserialize().ok()?;
            Signal::Token(id, token)
        }
        _ => return None,
    })
}
