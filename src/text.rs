use std::sync::LazyLock;

use regex::Regex;

static TAGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<script\b.*?</script>|<style\b.*?</style>|<[^>]*(?:>|$)").unwrap()
});
static TRACKING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:utm_|fbclid$|gclid$|mc_|ref$|source$)").unwrap());

pub fn plain(s: &str) -> String {
    let stripped = TAGS.replace_all(s, " ");
    htmlize::unescape(stripped.as_ref())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn thin(content: &str) -> bool {
    !content.contains("<img") && plain(content).chars().count() < 500
}

pub fn escape(s: &str) -> String {
    htmlize::escape_attribute(s).into_owned()
}

/// Python's `urlparse` split: (netloc, path, query), with `;params` cut from the path.
fn split_url(url: &str) -> (&str, &str, &str) {
    let url = url.split_once('#').map_or(url, |(u, _)| u);
    let (rest, query) = url.split_once('?').unwrap_or((url, ""));
    let rest = match rest.find(':') {
        Some(i)
            if i > 0
                && rest[..i].starts_with(|c: char| c.is_ascii_alphabetic())
                && rest[..i]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) =>
        {
            &rest[i + 1..]
        }
        _ => rest,
    };
    let (netloc, path) = match rest.strip_prefix("//") {
        Some(r) => r.find('/').map_or((r, ""), |i| r.split_at(i)),
        None => ("", rest),
    };
    let last = path.rfind('/').map_or(0, |i| i + 1);
    let path = path[last..].find(';').map_or(path, |i| &path[..last + i]);
    (netloc, path, query)
}

pub fn norm_link(url: &str) -> Option<String> {
    let (netloc, path, query) = split_url(url);
    if path.trim_matches('/').is_empty() {
        return None;
    }
    let q: Vec<&str> = query
        .split('&')
        .filter(|kv| !kv.is_empty() && !TRACKING.is_match(kv.split('=').next().unwrap_or("")))
        .collect();
    let host = netloc.to_lowercase();
    let mut out =
        host.strip_prefix("www.").unwrap_or(&host).to_owned() + path.trim_end_matches('/');
    if !q.is_empty() {
        out = out + "?" + &q.join("&");
    }
    Some(out)
}

pub fn summary(rows: &[(String, String)]) -> (String, String) {
    let feed = &rows[0].1;
    let one_feed = rows.iter().all(|(_, f)| f == feed);
    let n = rows.len();
    let head = if n == 1 {
        feed.clone()
    } else if one_feed {
        format!("{n} new from {feed}")
    } else {
        format!("{n} new articles")
    };
    let body = if n == 1 {
        plain(&rows[0].0)
    } else if one_feed {
        String::new()
    } else {
        let mut feeds: Vec<&str> = Vec::new();
        for (_, f) in rows {
            if !feeds.contains(&f.as_str()) {
                feeds.push(f);
            }
        }
        let more = feeds.len().saturating_sub(2);
        let mut b = feeds[..feeds.len().min(2)].join(" · ");
        if more > 0 {
            b += &format!(" · +{more} more");
        }
        b
    };
    (head, body)
}

/// Python's `str.capitalize`.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| {
        f.to_uppercase()
            .chain(c.flat_map(char::to_lowercase))
            .collect()
    })
}

pub type Opml = Vec<(String, Option<String>, Option<String>)>;

pub fn read_opml(data: &str) -> Result<Opml, roxmltree::Error> {
    fn walk(el: roxmltree::Node, folder: Option<&str>, out: &mut Opml) {
        for o in el.children().filter(|n| n.has_tag_name("outline")) {
            let name = o
                .attribute("title")
                .filter(|s| !s.is_empty())
                .or_else(|| o.attribute("text"));
            match o.attribute("xmlUrl").filter(|s| !s.is_empty()) {
                Some(url) => out.push((url.into(), name.map(Into::into), folder.map(Into::into))),
                None => walk(o, name, out),
            }
        }
    }
    let doc = roxmltree::Document::parse(data)?;
    let mut out = Vec::new();
    if let Some(body) = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("body"))
    {
        walk(body, None, &mut out);
    }
    Ok(out)
}

pub fn write_opml(feeds: &[(String, String, Option<String>, Option<String>)]) -> String {
    enum Node<'a> {
        Feed(String),
        Folder(&'a str, String),
    }
    let mut nodes: Vec<Node> = Vec::new();
    for (url, title, site, folder) in feeds {
        let (url, title) = (escape(url), escape(title));
        let site = site
            .as_deref()
            .map(|s| format!(" htmlUrl=\"{}\"", escape(s)))
            .unwrap_or_default();
        let line = format!(
            "<outline type=\"rss\" text=\"{title}\" title=\"{title}\" xmlUrl=\"{url}\"{site} />"
        );
        let Some(f) = folder.as_deref().filter(|f| !f.is_empty()) else {
            nodes.push(Node::Feed(line));
            continue;
        };
        match nodes.iter_mut().find_map(|n| match n {
            Node::Folder(name, inner) if *name == f => Some(inner),
            _ => None,
        }) {
            Some(inner) => inner.push_str(&line),
            None => nodes.push(Node::Folder(f, line)),
        }
    }
    let body: String = nodes
        .into_iter()
        .map(|n| match n {
            Node::Feed(line) => line,
            Node::Folder(f, inner) => {
                let f = escape(f);
                format!("<outline text=\"{f}\" title=\"{f}\">{inner}</outline>")
            }
        })
        .collect();
    format!(
        "<?xml version='1.0' encoding='UTF-8'?>\n<opml version=\"2.0\"><head><title>Tidings</title></head><body>{body}</body></opml>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_strips_tags_and_entities() {
        assert_eq!(plain("<p>a<script>x</script> &amp; <b>b</b></p>"), "a & b");
        assert_eq!(plain("a <img src=\"x"), "a");
        assert!(thin("<p>short</p>") && !thin("<img src=x>"));
    }

    #[test]
    fn norm_link_drops_tracking() {
        assert_eq!(
            norm_link("https://www.Ex.com/news/1/?utm_source=a&id=2#x").as_deref(),
            Some("ex.com/news/1?id=2")
        );
        assert_eq!(norm_link("https://ex.com/"), None);
        assert_eq!(norm_link(""), None);
        assert_eq!(
            norm_link("https://ex.com/a;p?ref=x").as_deref(),
            Some("ex.com/a")
        );
    }

    #[test]
    fn summaries() {
        let r = |t: &str, f: &str| (t.to_owned(), f.to_owned());
        assert_eq!(summary(&[r("<b>A</b>", "F")]), ("F".into(), "A".into()));
        assert_eq!(
            summary(&[r("A", "F"), r("B", "F")]),
            ("2 new from F".into(), String::new())
        );
        assert_eq!(
            summary(&[r("A", "F"), r("B", "G"), r("C", "G"), r("D", "G")]),
            ("4 new articles".into(), "F · G".into())
        );
        assert_eq!(
            summary(&[r("A", "F"), r("B", "G"), r("C", "H"), r("D", "I")]),
            ("4 new articles".into(), "F · G · +2 more".into())
        );
    }

    #[test]
    fn opml_round_trip() {
        let s = |v: &str| Some(v.to_owned());
        let feeds = vec![
            (
                "https://a.org/rss".into(),
                "A & B".into(),
                s("https://a.org"),
                None,
            ),
            ("https://c.org/atom".into(), "C".into(), None, s("Tech")),
            ("https://e.org/".into(), "E".into(), None, None),
            ("https://d.org/".into(), "D".into(), None, s("Tech")),
        ];
        let back = read_opml(&write_opml(&feeds)).unwrap();
        let mut want: Opml = feeds
            .into_iter()
            .map(|(u, t, _, f)| (u, Some(t), f))
            .collect();
        want.swap(2, 3);
        assert_eq!(back, want);
    }

    #[test]
    fn capitalizes_like_python() {
        assert_eq!(capitalize("monday, 12 November"), "Monday, 12 november");
    }
}
