use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, SystemTime};

use aho_corasick::AhoCorasick;
use regex::Regex;

const LISTS: [&str; 3] = [
    "https://easylist.to/easylist/easylist.txt",
    "https://easylist.to/easylist/easyprivacy.txt",
    "https://secure.fanboy.co.nz/fanboy-cookiemonster.txt",
];
const HOST_OPTS: [&str; 10] = [
    "third-party",
    "3p",
    "script",
    "image",
    "xmlhttprequest",
    "subdocument",
    "ping",
    "media",
    "websocket",
    "other",
];

static HOST_RULE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(@@)?\|\|([a-z0-9.-]+)\^(?:\$([a-z,-]+))?$").unwrap());
static PATH_RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/[\w./-]+\|?$").unwrap());
static COSMETIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([a-z0-9.,-]*)##(.+)$").unwrap());
static NAV_TRACKING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:utm_|fbclid=|gclid=|mc_[ce]id=)").unwrap());

#[derive(Default)]
pub struct Lists {
    hosts: HashSet<String>,
    allow: HashSet<String>,
    paths: Option<AhoCorasick>,
    pub css: BTreeMap<String, String>,
}

static ACTIVE: RwLock<Option<Lists>> = RwLock::new(None);

pub fn parse_lists(texts: &[String]) -> Lists {
    // ponytail: Ad-Shield anti-adblock config host, missing from lists
    let mut l = Lists {
        hosts: HashSet::from(["content-settings.com".into()]),
        ..Default::default()
    };
    l.css.insert(String::new(), String::new());
    let mut paths = Vec::new();
    for (i, text) in texts.iter().enumerate() {
        let cookies = i == texts.len() - 1;
        for line in text.lines() {
            if let Some(m) = HOST_RULE.captures(line)
                && m.get(3)
                    .is_none_or(|o| o.as_str().split(',').all(|o| HOST_OPTS.contains(&o)))
            {
                let set = if m.get(1).is_some() {
                    &mut l.allow
                } else {
                    &mut l.hosts
                };
                set.insert(m[2].to_owned());
            } else if let Some(m) = COSMETIC
                .captures(line)
                .filter(|m| cookies || m[1].is_empty())
            {
                for d in m[1].split(',') {
                    let rule = format!("{}{{display:none!important}}", &m[2]);
                    l.css.entry(d.to_owned()).or_default().push_str(&rule);
                }
            } else if cookies && PATH_RULE.is_match(line) {
                paths.push(line.trim_end_matches('|').to_owned());
            }
        }
    }
    l.paths = (!paths.is_empty()).then(|| AhoCorasick::new(paths).unwrap());
    l
}

impl Lists {
    pub fn blocks(&self, host: &str, path: &str) -> bool {
        let p: Vec<&str> = host.split('.').collect();
        let ds: Vec<String> = (0..p.len().saturating_sub(1))
            .map(|i| p[i..].join("."))
            .collect();
        !ds.iter().any(|d| self.allow.contains(d))
            && (ds.iter().any(|d| self.hosts.contains(d))
                || self.paths.as_ref().is_some_and(|ac| ac.is_match(path)))
    }

    fn script(&self) -> String {
        format!(
            "(() => {{ const m = {}, h = location.hostname.split(\".\"), s = new CSSStyleSheet(); let c = m[\"\"]\
             ; for (let i = 0; i < h.length - 1; i++) c += m[h.slice(i).join(\".\")] || \"\"\
             ; s.replaceSync(c); document.adoptedStyleSheets = [...document.adoptedStyleSheets, s] }})()",
            serde_json::to_string(&self.css).unwrap()
        )
    }
}

/// Downloads stale lists, activates them, and returns the cosmetic-filter user script.
pub fn load(folder: &Path) -> String {
    let texts: Vec<String> = LISTS
        .iter()
        .map(|url| {
            let path = folder.join(url.rsplit('/').next().unwrap());
            let stale = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .map_or(true, |t| {
                    SystemTime::now().duration_since(t).unwrap_or_default()
                        > Duration::from_secs(4 * 86400)
                });
            let refreshed = if stale {
                crate::web::get(url, u64::MAX)
                    .and_then(|p| std::fs::write(&path, p.body).map_err(|e| e.to_string()))
            } else {
                Ok(())
            };
            refreshed
                .and_then(|()| std::fs::read(&path).map_err(|e| e.to_string()))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_else(|e| {
                    eprintln!("blocklist {url}: {e}");
                    String::new()
                })
        })
        .collect();
    let lists = parse_lists(&texts);
    let js = lists.script();
    *ACTIVE.write().unwrap() = Some(lists);
    js
}

pub fn blocks(host: &str, path: &str) -> bool {
    ACTIVE
        .read()
        .unwrap()
        .as_ref()
        .is_some_and(|l| l.blocks(host, path))
}

/// The query without navigation trackers, or None when nothing was stripped.
pub fn clean_query(query: &str) -> Option<String> {
    let q: Vec<&str> = query.split('&').collect();
    q.iter().any(|kv| NAV_TRACKING.is_match(kv)).then(|| {
        q.into_iter()
            .filter(|kv| !NAV_TRACKING.is_match(kv))
            .collect::<Vec<_>>()
            .join("&")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rules() {
        let l = parse_lists(&[
            "||ads.com^\n||cdn.com^$script\n||odd.com^$popup\n@@||ok.ads.com^\n##.banner\nsite.com##.x".into(),
            "/track/pixel.gif|\nex.org##.cookie".into(),
        ]);
        assert!(
            l.blocks("a.ads.com", "/") && !l.blocks("ok.ads.com", "/") && l.blocks("cdn.com", "/")
        );
        assert!(
            !l.blocks("odd.com", "/")
                && l.blocks("x.org", "/a/track/pixel.gif?x")
                && !l.blocks("x.org", "/a")
        );
        assert!(l.blocks("content-settings.com", "/"));
        assert_eq!(l.css[""], ".banner{display:none!important}");
        assert_eq!(l.css["ex.org"], ".cookie{display:none!important}");
        assert!(!l.css.contains_key("site.com"));
    }

    #[test]
    fn strips_nav_trackers() {
        assert_eq!(
            clean_query("a=1&utm_source=x&fbclid=2").as_deref(),
            Some("a=1")
        );
        assert_eq!(clean_query("utm_x=1").as_deref(), Some(""));
        assert_eq!(clean_query("a=1"), None);
    }
}
