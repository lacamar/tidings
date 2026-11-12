use std::sync::{Arc, LazyLock, RwLock};

use fancy_regex::Regex as FancyRegex;
use feed_rs::model::{Entry, Feed};
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, functions::FunctionFlags, params};

use crate::text::norm_link;
use crate::web::{sanitize, urljoin};

pub const KEEP_DAYS: i64 = 60;

pub struct Rule {
    pub feed: Option<i64>,
    pub title_only: bool,
    pub rx: FancyRegex,
}

pub type Rules = Arc<RwLock<Vec<Rule>>>;

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub fn muted(rules: &[Rule], fid: i64, title: &str, content: &str) -> bool {
    rules.iter().any(|r| {
        r.feed.is_none_or(|f| f == fid)
            && (r.rx.is_match(title).unwrap_or(false)
                || !r.title_only && r.rx.is_match(content).unwrap_or(false))
    })
}

pub fn mute_rule(text: &str) -> Option<FancyRegex> {
    let words: Vec<String> = text
        .split(',')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(fancy_regex::escape)
        .map(Into::into)
        .collect();
    FancyRegex::new(&format!(r"(?i)(?<!\w)(?:{})(?!\w)", words.join("|"))).ok()
}

pub fn set_mute_fn(db: &Connection, rules: Rules) -> rusqlite::Result<()> {
    db.create_scalar_function("muted", 3, FunctionFlags::SQLITE_UTF8, move |ctx| {
        let fid = ctx.get::<Option<i64>>(0)?.unwrap_or(0);
        let title = ctx.get::<Option<String>>(1)?.unwrap_or_default();
        let content = ctx.get::<Option<String>>(2)?.unwrap_or_default();
        Ok(i64::from(muted(
            &rules.read().unwrap(),
            fid,
            &title,
            &content,
        )))
    })
}

pub fn open_db(path: &str, rules: Rules) -> rusqlite::Result<Connection> {
    let db = Connection::open(path)?;
    db.execute_batch(
        "pragma foreign_keys = on;
        create table if not exists feeds(id integer primary key, url text unique, title text, etag text,
            modified text, fetched integer, web integer default 0);
        create table if not exists items(id integer primary key, feed_id integer references feeds on delete cascade,
            guid text, title text, link text, author text, published integer, content text, image text,
            read integer default 0, unique(feed_id, guid));
        create index if not exists items_published on items(published);
        create table if not exists mutes(id integer primary key, text text, feed_id integer references feeds on delete cascade,
            title_only integer, expires integer);",
    )?;
    for (table, col) in [
        ("feeds", "site text"),
        ("feeds", "icon text"),
        ("feeds", "name text"),
        ("feeds", "error text"),
        ("feeds", "notify integer default 1"),
        ("items", "full text"),
        ("items", "starred integer default 0"),
        ("items", "media text"),
        ("items", "media_type text"),
        ("feeds", "folder text"),
        ("items", "pos real"),
        ("feeds", "fulltext integer default 0"),
        ("items", "nlink text"),
        ("items", "dup integer"),
        ("items", "muted integer default 0"),
    ] {
        let _ = db.execute(&format!("alter table {table} add column {col}"), []);
    }
    db.execute("create index if not exists items_nlink on items(nlink)", [])?;
    set_mute_fn(&db, rules)?;
    db.create_scalar_function(
        "norm_link",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| Ok(ctx.get::<Option<String>>(0)?.as_deref().and_then(norm_link)),
    )?;
    db.execute(
        "update items set nlink = norm_link(link) where nlink is null and link is not null",
        [],
    )?;
    Ok(db)
}

pub enum Fetch {
    NotModified,
    Feed {
        feed: Box<Feed>,
        etag: Option<String>,
        modified: Option<String>,
    },
}

pub fn link_of(links: &[feed_rs::model::Link]) -> Option<&str> {
    links
        .iter()
        .find(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
        .map(|l| l.href.as_str())
}

static IMG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<img[^>]+src=["']([^"']+)"#).unwrap());

pub fn image_of(e: &Entry, content: &str) -> Option<String> {
    let thumbs = e
        .media
        .iter()
        .flat_map(|m| m.thumbnails.iter().map(|t| t.image.uri.clone()));
    let images = e.media.iter().flat_map(|m| &m.content).filter_map(|c| {
        c.content_type
            .as_ref()
            .filter(|t| t.ty().as_str() == "image")
            .and(c.url.as_ref())
            .map(|u| u.to_string())
    });
    if let Some(u) = thumbs.chain(images).find(|u| !u.is_empty()) {
        return Some(u);
    }
    IMG.captures(content)
        .map(|m| urljoin(link_of(&e.links).unwrap_or(""), &htmlize::unescape(&m[1])))
}

pub fn media_of(e: &Entry) -> (Option<String>, Option<String>) {
    if let Some(id) = e.id.strip_prefix("yt:video:") {
        return (
            Some(format!("https://www.youtube-nocookie.com/embed/{id}")),
            Some("youtube".into()),
        );
    }
    e.media
        .iter()
        .flat_map(|m| &m.content)
        .find_map(|c| {
            let ty = c.content_type.as_ref()?.ty().as_str().to_owned();
            matches!(ty.as_str(), "audio" | "video")
                .then_some((Some(c.url.as_ref()?.to_string()), Some(ty)))
        })
        .unwrap_or((None, None))
}

fn content_of(e: &Entry) -> String {
    let html = |ty: &mediatype::MediaTypeBuf| {
        matches!(ty.subty().as_str(), "html" | "xhtml+xml" | "xhtml")
    };
    if let Some(c) = &e.content
        && let Some(body) = &c.body
    {
        return if html(&c.content_type) {
            sanitize(body)
        } else {
            body.clone()
        };
    }
    e.summary
        .as_ref()
        .map(|s| {
            if html(&s.content_type) {
                sanitize(&s.content)
            } else {
                s.content.clone()
            }
        })
        .unwrap_or_default()
}

pub fn store(db: &Connection, feed_id: i64, fetch: &Fetch) -> rusqlite::Result<Vec<i64>> {
    let Fetch::Feed {
        feed,
        etag,
        modified,
    } = fetch
    else {
        return Ok(Vec::new());
    };
    // pruned items must not come back as unread; a new feed still gets a taste of its backlog
    let seen = db
        .query_row("select fetched from feeds where id = ?", [feed_id], |r| {
            r.get::<_, Option<i64>>(0)
        })?
        .is_some_and(|f| f != 0);
    let now = now();
    let cutoff = now - KEEP_DAYS * 86400;
    let tx = db.unchecked_transaction()?;
    tx.execute(
        "update feeds set title = coalesce(?, title), site = coalesce(?, site), etag = ?, modified = ?, fetched = ? where id = ?",
        params![feed.title.as_ref().map(|t| &t.content), link_of(&feed.links), etag, modified, now, feed_id],
    )?;
    let mut new = Vec::new();
    for (n, e) in feed.entries.iter().enumerate() {
        let content = content_of(e);
        let t = e.published.or(e.updated).map(|t| t.timestamp());
        if t.is_some_and(|t| t < cutoff) && (seen || n >= 10) {
            continue;
        }
        let link = link_of(&e.links);
        let title = e
            .title
            .as_ref()
            .map(|t| t.content.as_str())
            .filter(|t| !t.is_empty());
        let guid = Some(e.id.as_str())
            .filter(|s| !s.is_empty())
            .or(link)
            .or(title)
            .unwrap_or(&content);
        let pub_ = t.unwrap_or(now);
        let (media, media_type) = media_of(e);
        let inserted = tx.execute(
            "insert or ignore into items(feed_id, guid, title, link, author, published, content, image, media, media_type)
             values(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                feed_id,
                guid,
                title.unwrap_or("Untitled"),
                link,
                e.authors.first().map(|a| &a.name),
                pub_,
                content,
                image_of(e, &content),
                media,
                media_type
            ],
        )?;
        if inserted == 0 {
            continue;
        }
        let i = tx.last_insert_rowid();
        let nl = link.and_then(norm_link);
        let title = title.unwrap_or("");
        // ponytail: exact link or 4+-word title within 3 days; fuzzy title match if near-dupes slip through
        let dup: Option<i64> = tx
            .query_row(
                "select id from items where id != ? and (nlink = ? or (? and title = ? collate nocase and abs(published - ?) < 259200))
                 order by id limit 1",
                params![i, nl, title.split_whitespace().count() >= 4, title, pub_],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute(
            "update items set nlink = ?, dup = ?, muted = muted(feed_id, title, content) where id = ?",
            params![nl, dup, i],
        )?;
        new.push(i);
    }
    tx.commit()?;
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::parse_feed;

    const RSS: &str = r#"<?xml version="1.0"?><rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel><title>T</title>
<item><guid>a</guid><title>One</title><link>https://x.org/1</link><description>&lt;p&gt;Hi &lt;img src="/i.png"&gt;&lt;/p&gt;</description></item>
<item><guid>b</guid><title>Two</title><link>https://x.org/2</link><media:thumbnail url="https://x.org/t.jpg"/></item>
</channel></rss>"#;

    fn feed(xml: &str) -> Fetch {
        Fetch::Feed {
            feed: Box::new(parse_feed(xml.as_bytes(), None).unwrap()),
            etag: None,
            modified: None,
        }
    }

    fn mem() -> Connection {
        open_db(":memory:", Rules::default()).unwrap()
    }

    fn add(db: &Connection, url: &str, fetched: Option<i64>) -> i64 {
        db.execute(
            "insert into feeds(url, fetched) values(?, ?)",
            params![url, fetched],
        )
        .unwrap();
        db.last_insert_rowid()
    }

    #[test]
    fn stores_new_items_once() {
        let db = mem();
        let fid = add(&db, "u", None);
        assert_eq!(store(&db, fid, &feed(RSS)).unwrap().len(), 2);
        assert!(store(&db, fid, &feed(RSS)).unwrap().is_empty());
        let title: String = db
            .query_row("select title from feeds", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "T");
        let mut imgs: Vec<(String, String)> = db
            .prepare("select guid, image from items order by guid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        imgs.sort();
        assert_eq!(
            imgs,
            [
                ("a".into(), "https://x.org/i.png".into()),
                ("b".into(), "https://x.org/t.jpg".into())
            ]
        );

        let old = RSS.replace(
            "<guid>b</guid>",
            "<guid>c</guid><pubDate>Mon, 01 Jan 2001 00:00:00 GMT</pubDate>",
        );
        assert!(store(&db, fid, &feed(&old)).unwrap().is_empty());
    }

    #[test]
    fn new_feed_gets_ten_old_items() {
        let db = mem();
        let fid = add(&db, "v", None);
        let items: String = (0..15)
            .map(|i| {
                format!(
                    "<item><guid>{i}</guid><pubDate>Mon, 01 Jan 2001 00:00:00 GMT</pubDate></item>"
                )
            })
            .collect();
        let backlog = RSS.replace("</channel>", &(items + "</channel>"));
        assert_eq!(store(&db, fid, &feed(&backlog)).unwrap().len(), 10);
    }

    #[test]
    fn media() {
        let yt = feed(
            r#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><id>yt:video:abc</id><title>v</title></entry></feed>"#,
        );
        let Fetch::Feed { feed: yt, .. } = yt else {
            unreachable!()
        };
        assert_eq!(
            media_of(&yt.entries[0]),
            (
                Some("https://www.youtube-nocookie.com/embed/abc".into()),
                Some("youtube".into())
            )
        );
        let pod = feed(
            r#"<rss version="2.0"><channel><item><title>p</title><enclosure url="https://x.org/a.mp3" type="audio/mpeg" length="1"/></item></channel></rss>"#,
        );
        let Fetch::Feed { feed: pod, .. } = pod else {
            unreachable!()
        };
        assert_eq!(
            media_of(&pod.entries[0]),
            (Some("https://x.org/a.mp3".into()), Some("audio".into()))
        );
    }

    #[test]
    fn dups_and_mutes() {
        let db = mem();
        let rules = Rules::default();
        rules.write().unwrap().push(Rule {
            feed: None,
            title_only: true,
            rx: mute_rule("giveaway").unwrap(),
        });
        set_mute_fn(&db, rules).unwrap();
        let items = |e: &[(&str, &str, &str)]| {
            let i: String = e
                .iter()
                .map(|(g, t, u)| {
                    format!("<item><guid>{g}</guid><title>{t}</title><link>{u}</link></item>")
                })
                .collect();
            feed(&format!(
                "<rss version=\"2.0\"><channel><title>x</title>{i}</channel></rss>"
            ))
        };
        let (a, b) = (add(&db, "a", Some(1)), add(&db, "b", Some(1)));
        store(
            &db,
            a,
            &items(&[
                (
                    "1",
                    "Big news about the thing",
                    "https://www.ex.com/news/1?utm_source=a",
                ),
                ("2", "Weekly giveaway", "https://ex.com/g"),
            ]),
        )
        .unwrap();
        store(
            &db,
            b,
            &items(&[
                ("9", "Other title", "https://ex.com/news/1/"),
                ("8", "big news about the thing", "https://other.org/x"),
                ("7", "Fresh", "https://ex.com/f"),
            ]),
        )
        .unwrap();
        let mut got: Vec<(String, bool, i64)> = db
            .prepare("select title, dup is not null, muted from items")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        got.sort();
        let want = [
            ("Big news about the thing", false, 0),
            ("Fresh", false, 0),
            ("Other title", true, 0),
            ("Weekly giveaway", false, 1),
            ("big news about the thing", true, 0),
        ];
        assert_eq!(got, want.map(|(t, d, m)| (t.to_owned(), d, m)));
    }

    #[test]
    fn mute_rule_matches_words() {
        let rx = mute_rule(" foo , #bar,").unwrap();
        assert!(rx.is_match("a FOO b").unwrap() && rx.is_match("x #bar").unwrap());
        assert!(!rx.is_match("food").unwrap() && !rx.is_match("a#barb").unwrap());
    }
}
