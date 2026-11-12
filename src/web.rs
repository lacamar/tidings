use std::io::Read;
use std::sync::LazyLock;
use std::time::Duration;

use feed_rs::model::Feed;
use kuchikiki::NodeRef;
use kuchikiki::traits::{NodeIterator, TendrilSink};
use regex::Regex;
use ureq::{Agent, ResponseExt};

use crate::db::Fetch;
use crate::text::plain;

pub const UA: &str = "Mozilla/5.0 (X11; Linux) Tidings/0.1";

static AGENT: LazyLock<Agent> = LazyLock::new(|| {
    Agent::config_builder()
        .user_agent(UA)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(20)))
        .timeout_recv_body(Some(Duration::from_secs(60)))
        .build()
        .into()
});

static SANITIZER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut b = ammonia::Builder::default();
    b.add_tags(["audio", "video", "source", "picture"])
        .add_tag_attributes("audio", ["src", "controls"])
        .add_tag_attributes("video", ["src", "controls", "poster"])
        .add_tag_attributes("source", ["src", "srcset", "type"])
        .add_tag_attributes("img", ["srcset"]);
    b
});

pub fn sanitize(html: &str) -> String {
    SANITIZER.clean(html).to_string()
}

pub fn urljoin(base: &str, rel: &str) -> String {
    url::Url::parse(base)
        .and_then(|b| b.join(rel))
        .map_or_else(|_| rel.to_owned(), String::from)
}

pub struct Page {
    pub url: String,
    pub body: Vec<u8>,
    pub charset: Option<String>,
}

fn request(
    url: &str,
    limit: u64,
    headers: &[(&str, &str)],
) -> Result<(u16, Page, Option<String>, Option<String>), String> {
    let mut req = AGENT.get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut r = req.call().map_err(|e| e.to_string())?;
    let header = |k: &str| {
        r.headers()
            .get(k)
            .and_then(|v| v.to_str().ok())
            .map(String::from)
    };
    let (etag, modified) = (header("etag"), header("last-modified"));
    let status = r.status().as_u16();
    let final_url = r.get_uri().to_string();
    let charset = r.body().charset().map(String::from);
    let mut body = Vec::new();
    r.body_mut()
        .with_config()
        .limit(u64::MAX)
        .reader()
        .take(limit)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    Ok((
        status,
        Page {
            url: final_url,
            body,
            charset,
        },
        etag,
        modified,
    ))
}

pub fn get(url: &str, limit: u64) -> Result<Page, String> {
    let (status, page, ..) = request(url, limit, &[])?;
    if status >= 400 {
        return Err(format!("HTTP Error {status}"));
    }
    Ok(page)
}

pub fn page(url: &str, limit: u64) -> Result<(String, String), String> {
    let p = get(url, limit)?;
    let enc = p
        .charset
        .as_deref()
        .and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    Ok((
        p.url,
        enc.decode_without_bom_handling(&p.body).0.into_owned(),
    ))
}

pub fn parse_feed(data: &[u8], base: Option<&str>) -> Result<Feed, String> {
    feed_rs::parser::Builder::new()
        .base_uri(base)
        .id_generator(|links, title, _| {
            crate::db::link_of(links)
                .map(String::from)
                .or_else(|| title.as_ref().map(|t| t.content.clone()))
                .unwrap_or_default()
        })
        .build()
        .parse(data)
        .map_err(|e| e.to_string())
}

pub fn fetch_feed(url: &str, etag: Option<&str>, modified: Option<&str>) -> Result<Fetch, String> {
    let mut headers = vec![(
        "Accept",
        "application/atom+xml,application/rdf+xml,application/rss+xml,application/xml,text/xml;q=0.9,*/*;q=0.1",
    )];
    headers.extend(etag.map(|e| ("If-None-Match", e)));
    headers.extend(modified.map(|m| ("If-Modified-Since", m)));
    let (status, page, etag, modified) = request(url, u64::MAX, &headers)?;
    if status == 304 {
        return Ok(Fetch::NotModified);
    }
    match parse_feed(&page.body, Some(&page.url)) {
        Ok(feed) => Ok(Fetch::Feed {
            feed: Box::new(feed),
            etag,
            modified,
        }),
        Err(_) if status >= 400 => Err(format!("HTTP {status}")),
        Err(e) => Err(e),
    }
}

fn fetch_as_feed(url: &str) -> Option<Fetch> {
    fetch_feed(url, None, None)
        .ok()
        .filter(|f| matches!(f, Fetch::Feed { .. }))
}

pub struct Link {
    pub href: String,
    pub rel: Vec<String>,
    pub ty: String,
    pub sizes: String,
}

pub fn links(html: &str) -> Vec<Link> {
    let doc = kuchikiki::parse_html().one(html);
    doc.select("link[href]")
        .unwrap()
        .filter_map(|el| {
            let a = el.attributes.borrow();
            let href = a.get("href").filter(|h| !h.is_empty())?.to_owned();
            let get = |k| a.get(k).unwrap_or("").to_owned();
            Some(Link {
                href,
                rel: get("rel")
                    .to_lowercase()
                    .split_whitespace()
                    .map(String::from)
                    .collect(),
                ty: get("type"),
                sizes: get("sizes"),
            })
        })
        .collect()
}

pub fn find(links: &[Link], rel: &str, ty: &Regex) -> Vec<String> {
    links
        .iter()
        .filter(|l| l.rel.iter().any(|r| r == rel) && ty.is_match(&l.ty))
        .map(|l| l.href.clone())
        .collect()
}

static ANY: LazyLock<Regex> = LazyLock::new(|| Regex::new("").unwrap());
static FEED_TYPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(rss|atom)\+xml").unwrap());
static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+)x").unwrap());

pub const NO_FEED: &str = "no feed found there";

/// Err carries the message the add dialog shows.
pub fn discover(url: &str) -> Result<(String, Fetch), String> {
    let url = if url.contains("://") {
        url.to_owned()
    } else {
        format!("https://{url}")
    };
    if let Some(f) = fetch_as_feed(&url) {
        return Ok((url, f));
    }
    let (base, text) =
        page(&url, 2_000_000).map_err(|e| format!("Couldn't load that address ({e})"))?;
    let here = base
        .split('?')
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_owned();
    let mut guesses = find(&links(&text), "alternate", &FEED_TYPE);
    guesses.truncate(1);
    if guesses.is_empty() {
        guesses = [format!("{here}/.rss"), format!("{here}/releases.atom")]
            .into_iter()
            .chain(
                [
                    "/feed",
                    "/rss",
                    "/feed.xml",
                    "/atom.xml",
                    "/rss.xml",
                    "/index.xml",
                ]
                .map(String::from),
            )
            .collect();
    }
    for guess in guesses {
        let url = urljoin(&base, &guess);
        if let Some(f) = fetch_as_feed(&url) {
            return Ok((url, f));
        }
    }
    Err(NO_FEED.into())
}

/// `save` decodes, scales and writes the image; false when it is unusable.
pub fn favicon(site: &str, save: impl Fn(&[u8]) -> bool) -> Result<(), String> {
    let (base, text) = page(site, 1_000_000)?;
    let all = links(&text);
    let size = |l: &Link| {
        SIZE.captures_iter(&l.sizes)
            .filter_map(|c| c[1].parse::<i64>().ok())
            .max()
            .unwrap_or(0)
    };
    let mut sized: Vec<&Link> = all
        .iter()
        .filter(|l| l.rel.iter().any(|r| r == "icon"))
        .collect();
    sized.sort_by_key(|l| -size(l));
    let hrefs = find(&all, "apple-touch-icon", &ANY)
        .into_iter()
        .chain(sized.into_iter().map(|l| l.href.clone()))
        .chain(["/favicon.ico".to_owned()]);
    for href in hrefs {
        if let Ok(img) = get(&urljoin(&base, &href), 2_000_000)
            && save(&img.body)
        {
            return Ok(());
        }
    }
    Err("no icon".into())
}

const DROP: &str = "script, style, noscript, nav, header, footer, aside, form, button, svg";
const LINK_ATTRS: [&str; 4] = ["href", "src", "poster", "cite"];

fn attr(n: &NodeRef, k: &str) -> Option<String> {
    n.as_element()?.attributes.borrow().get(k).map(String::from)
}

pub fn extract_html(base: &str, text: &str) -> (String, Option<String>) {
    let doc = kuchikiki::parse_html().one(text);
    let og = doc
        .select(r#"meta[property="og:image"], meta[name="twitter:image"]"#)
        .unwrap()
        .next()
        .and_then(|m| m.attributes.borrow().get("content").map(String::from))
        .filter(|s| !s.is_empty());
    for el in doc.select(DROP).unwrap().collect::<Vec<_>>() {
        el.as_node().detach();
    }
    for img in doc.select("img").unwrap() {
        let mut a = img.attributes.borrow_mut();
        if a.get("src")
            .is_none_or(|s| s.is_empty() || s.starts_with("data:"))
        {
            let src = a
                .get("data-src")
                .filter(|s| !s.is_empty())
                .map(String::from)
                .unwrap_or_else(|| {
                    let set = a
                        .get("srcset")
                        .filter(|s| !s.is_empty())
                        .or_else(|| a.get("data-srcset"))
                        .unwrap_or("");
                    set.split(' ').next().unwrap_or("").to_owned()
                });
            a.insert("src", src);
        }
    }
    for el in doc.descendants().elements() {
        let mut a = el.attributes.borrow_mut();
        for k in LINK_ATTRS {
            if let Some(v) = a.get_mut(k) {
                *v = urljoin(base, v.trim());
            }
        }
    }
    // ponytail: densest-<p> heuristic, swap in a readability port if pages extract badly
    let mut score: Vec<(NodeRef, f64)> = Vec::new();
    let mut add = |el: NodeRef, n: f64| match score.iter_mut().find(|(e, _)| *e == el) {
        Some((_, s)) => *s += n,
        None => score.push((el, n)),
    };
    for p in doc.select("p, pre").unwrap() {
        let n = p.text_contents().trim().chars().count() as f64;
        let element = |n: &NodeRef| n.as_element().is_some();
        if let Some(parent) = p.as_node().parent().filter(element) {
            add(parent.clone(), n);
            if let Some(gp) = parent.parent().filter(element) {
                add(gp, n * 0.5);
            }
        }
    }
    let best = score.iter().fold(None::<&(NodeRef, f64)>, |m, e| match m {
        Some(m) if m.1 >= e.1 => Some(m),
        _ => Some(e),
    });
    let parts: Vec<NodeRef> = match best {
        None => Vec::new(),
        Some((best, top)) => match attr(best, "class").filter(|c| !c.is_empty()) {
            None => vec![best.clone()],
            // articles split into same-class blocks around ads (Ars Technica)
            Some(class) => {
                let tag = best.as_element().unwrap().name.local.to_string();
                doc.select(&tag)
                    .unwrap()
                    .map(|e| e.as_node().clone())
                    .filter(|e| {
                        attr(e, "class").as_deref() == Some(class.as_str())
                            && score.iter().find(|(s, _)| s == e).map_or(0.0, |s| s.1) > top / 5.0
                    })
                    .collect()
            }
        },
    };
    let body = sanitize(&parts.iter().map(NodeRef::to_string).collect::<String>());
    let body = if plain(&body).chars().count() > 300 {
        body
    } else {
        String::new()
    };
    (body, og.map(|og| urljoin(base, &og)))
}

pub fn extract(url: &str) -> Result<(String, Option<String>), String> {
    let (base, text) = page(url, 4_000_000)?;
    Ok(extract_html(&base, &text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links() {
        let l = links(
            r#"<head><link rel="alternate" type="application/atom+xml" href="/feed"><link rel="Icon" href="/i.png"></head>"#,
        );
        assert_eq!(find(&l, "alternate", &FEED_TYPE), ["/feed"]);
        assert_eq!(find(&l, "icon", &ANY), ["/i.png"]);
    }

    #[test]
    fn extracts_split_articles() {
        let split: String = (0..3)
            .map(|i| {
                format!(
                    "<div><div class=\"post\"><p>part{i} {}</p></div><p>ad</p></div>",
                    "word ".repeat(80)
                )
            })
            .collect();
        let (body, _) = extract_html(
            "https://x.org/a",
            &format!("<html><body>{split}</body></html>"),
        );
        assert!((0..3).all(|i| body.contains(&format!("part{i}"))), "{body}");
        assert!(!plain(&body).split(' ').any(|w| w == "ad"));
        assert_eq!(
            extract_html(
                "https://x.org/a",
                "<html><body><p>nothing</p></body></html>"
            )
            .0,
            ""
        );
    }

    #[test]
    fn extract_resolves_og_and_lazy_images() {
        let html = format!(
            r#"<meta property="og:image" content="/og.png"><article><img data-src="/lazy.png" src="data:x"><p>{}</p></article>"#,
            "word ".repeat(80)
        );
        let (body, og) = extract_html("https://x.org/a/b", &html);
        assert_eq!(og.as_deref(), Some("https://x.org/og.png"));
        assert!(body.contains("https://x.org/lazy.png"), "{body}");
    }
}
