#!/usr/bin/env python3
import calendar
import html
import json
import os
import re
import secrets
import signal
import socket
import sqlite3
import subprocess
import sys
import threading
import time
import urllib.request
import xml.etree.ElementTree as ET
from concurrent.futures import ThreadPoolExecutor
from html.parser import HTMLParser
from queue import Queue
from urllib.parse import unquote, urljoin, urlparse
import feedparser
import lxml.html
from feedparser.sanitizer import _sanitize_html
from jeepney import DBusAddress, HeaderFields, MatchRule, message_bus, new_method_call
from jeepney.io import blocking
from jeepney.io.threading import DBusRouter, open_dbus_connection
from jeepney.wrappers import unwrap_msg
from PySide6.QtCore import Property, QDate, QDateTime, QLocale, QAbstractListModel, QModelIndex, QFileSystemWatcher, QObject, QSocketNotifier, Qt, QTimer, QUrl, Signal, Slot
from PySide6.QtGui import QColor, QGuiApplication, QIcon, QImage
from PySide6.QtNetwork import QLocalServer, QLocalSocket
from PySide6.QtQml import QQmlApplicationEngine
from PySide6.QtQuick import QQuickWindow
from PySide6.QtWidgets import QApplication, QMenu, QSystemTrayIcon
from PySide6.QtWebEngineCore import QWebEngineUrlRequestInfo, QWebEngineUrlRequestInterceptor
from PySide6.QtWebEngineQuick import QQuickWebEngineProfile, QtWebEngineQuick

APP_ID = 'dev.lacamar.Tidings'
UA = 'Mozilla/5.0 (X11; Linux) Tidings/0.1'
REFRESH_SECS = 30 * 60
LIMIT = 300
KEEP_DAYS = 60
WANTS = os.path.join(os.environ.get('XDG_CONFIG_HOME') or os.path.expanduser('~/.config'), 'systemd/user/graphical-session.target.wants/tidings.service')

LISTS = ('https://easylist.to/easylist/easylist.txt', 'https://easylist.to/easylist/easyprivacy.txt',
         'https://secure.fanboy.co.nz/fanboy-cookiemonster.txt')
THEME = os.path.join(os.environ.get('XDG_CONFIG_HOME') or os.path.expanduser('~/.config'), 'tidings/theme.json')
PALETTE = {False: dict(bg='#ffffff', side='#f4f4f6', raised='#ffffff', fg='#1b1b20', sub='#6c6c76', line='#e6e6ea'),
           True: dict(bg='#1c1c1f', side='#151517', raised='#2a2a2e', fg='#f2f2f4', sub='#9d9da6', line='#2b2b30')}

socket.setdefaulttimeout(20)
feedparser.USER_AGENT = UA

STYLE = '''
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
'''


def open_db(path):
    db = sqlite3.connect(path)
    db.row_factory = sqlite3.Row
    db.executescript('''
        pragma foreign_keys = on;
        create table if not exists feeds(id integer primary key, url text unique, title text, etag text,
            modified text, fetched integer, web integer default 0);
        create table if not exists items(id integer primary key, feed_id integer references feeds on delete cascade,
            guid text, title text, link text, author text, published integer, content text, image text,
            read integer default 0, unique(feed_id, guid));
        create index if not exists items_published on items(published);
        create table if not exists mutes(id integer primary key, text text, feed_id integer references feeds on delete cascade,
            title_only integer, expires integer);
    ''')
    for table, col in (('feeds', 'site text'), ('feeds', 'icon text'), ('feeds', 'name text'), ('feeds', 'error text'),
                       ('feeds', 'notify integer default 1'), ('items', 'full text'), ('items', 'starred integer default 0'),
                       ('items', 'media text'), ('items', 'media_type text'), ('feeds', 'folder text'), ('items', 'pos real'), ('feeds', 'fulltext integer default 0'),
                       ('items', 'nlink text'), ('items', 'dup integer'), ('items', 'muted integer default 0')):
        try:
            db.execute(f'alter table {table} add column {col}')
        except sqlite3.OperationalError:
            pass
    db.execute('create index if not exists items_nlink on items(nlink)')
    db.create_function('muted', 3, lambda *a: 0)
    db.create_function('norm_link', 1, norm_link)
    db.execute('update items set nlink = norm_link(link) where nlink is null and link is not null')
    return db


TRACKING = re.compile(r'utm_|fbclid$|gclid$|mc_|ref$|source$')


def norm_link(url):
    p = urlparse(url or '')
    if p.path.strip('/') == '':
        return None
    q = '&'.join(kv for kv in p.query.split('&') if kv and not TRACKING.match(kv.split('=')[0]))
    return p.netloc.lower().removeprefix('www.') + p.path.rstrip('/') + (q and '?' + q)


def image_of(e, content):
    media = e.get('media_thumbnail', []) + [m for m in e.get('media_content', []) + e.get('enclosures', [])
                                            if m.get('type', '').startswith('image') or m.get('medium') == 'image']
    for m in media:
        if m.get('url') or m.get('href'):
            return m.get('url') or m.get('href')
    m = re.search(r'<img[^>]+src=["\']([^"\']+)', content)
    return m and urljoin(e.get('link', ''), html.unescape(m[1]))


def media_of(e):
    if e.get('yt_videoid'):
        return f'https://www.youtube-nocookie.com/embed/{e.yt_videoid}', 'youtube'
    for m in e.get('enclosures', []) + e.get('media_content', []):
        if (m.get('type') or '').startswith(('audio/', 'video/')) and (m.get('href') or m.get('url')):
            return m.get('href') or m.get('url'), m['type'].split('/')[0]
    return None, None


def store(db, feed_id, d):
    if d.get('status') == 304 or not d.get('version'):
        return []
    # pruned items must not come back as unread; a new feed still gets a taste of its backlog
    seen = db.execute('select fetched from feeds where id = ?', (feed_id,)).fetchone()[0]
    cutoff = time.time() - KEEP_DAYS * 86400
    db.execute('update feeds set title = coalesce(?, title), site = coalesce(?, site), etag = ?, modified = ?, fetched = ?'
               ' where id = ?', (d.feed.get('title'), d.feed.get('link'), d.get('etag'), d.get('modified'), int(time.time()), feed_id))
    new = []
    for n, e in enumerate(d.entries):
        content = e.content[0].value if e.get('content') else e.get('summary', '')
        t = e.get('published_parsed') or e.get('updated_parsed')
        if t and calendar.timegm(t) < cutoff and (seen or n >= 10):
            continue
        c = db.execute('insert or ignore into items(feed_id, guid, title, link, author, published, content, image, media, media_type)'
                       ' values(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)',
                       (feed_id, e.get('id') or e.get('link') or e.get('title') or content, e.get('title') or 'Untitled',
                        e.get('link'), e.get('author'), calendar.timegm(t) if t else int(time.time()), content,
                        image_of(e, content), *media_of(e)))
        if c.rowcount:
            i, nl, title, pub = c.lastrowid, norm_link(e.get('link')), e.get('title') or '', calendar.timegm(t) if t else int(time.time())
            # ponytail: exact link or 4+-word title within 3 days; fuzzy title match if near-dupes slip through
            dup = db.execute('select id from items where id != ? and (nlink = ? or (? and title = ? collate nocase and abs(published - ?) < 259200))'
                             ' order by id limit 1', (i, nl, len(title.split()) >= 4, title, pub)).fetchone()
            db.execute('update items set nlink = ?, dup = ?, muted = muted(feed_id, title, content) where id = ?', (nl, dup and dup[0], i))
            new.append(i)
    db.commit()
    return new


def summary(rows):
    feeds = {f for _, f in rows}
    n, feed = len(rows), rows[0][1]
    head = feed if n == 1 else f'{n} new from {feed}' if len(feeds) == 1 else f'{n} new articles'
    return head, '\n'.join(plain(t) if len(feeds) == 1 else f'{f}: {plain(t)}' for t, f in rows[:3])


class Links(HTMLParser):
    def __init__(self, page):
        super().__init__()
        self.links = []
        self.feed(page)

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag == 'link' and a.get('href'):
            self.links.append(dict(a, rel=(a.get('rel') or '').lower().split()))

    def find(self, rel, type=''):
        return [a['href'] for a in self.links if rel in a['rel'] and re.search(type, a.get('type') or '')]


def get(url, limit=None):
    with urllib.request.urlopen(urllib.request.Request(url, headers={'User-Agent': UA})) as r:
        return r.url, r.read(limit), r.headers.get_content_charset()


def page(url, limit):
    base, body, charset = get(url, limit)
    return base, body.decode(charset or 'utf-8', 'replace')


def thin(content):
    return '<img' not in content and len(plain(content)) < 500


def extract(url):
    base, text = page(url, 4_000_000)
    doc = lxml.html.document_fromstring(text, base_url=base)
    og = doc.xpath('string(//meta[@property="og:image" or @name="twitter:image"]/@content)')
    for el in doc.xpath('//script|//style|//noscript|//nav|//header|//footer|//aside|//form|//button|//svg'):
        el.drop_tree()
    for img in doc.iter('img'):
        if not img.get('src') or img.get('src').startswith('data:'):
            img.set('src', img.get('data-src') or (img.get('srcset') or img.get('data-srcset') or '').split(' ')[0])
    doc.make_links_absolute(base)
    # ponytail: densest-<p> heuristic, swap in readability-lxml if pages extract badly
    score = {}
    for p in doc.xpath('//p|//pre'):
        n = len(p.text_content().strip())
        parent = p.getparent()
        for el, w in ((parent, 1), (parent.getparent() if parent is not None else None, .5)):
            if el is not None:
                score[el] = score.get(el, 0) + n * w
    best = max(score, key=score.get) if score else None
    # articles split into same-class blocks around ads (Ars Technica)
    parts = [best] if best is None or not best.get('class') else [
        el for el in doc.iter(best.tag) if el.get('class') == best.get('class') and score.get(el, 0) > score[best] / 5]
    body = _sanitize_html(''.join(lxml.html.tostring(el, encoding='unicode', with_tail=False) for el in parts if el is not None),
                          'utf-8', 'text/html')
    return (body if len(plain(body)) > 300 else ''), og and urljoin(base, og)


def favicon(site, path):
    base, text = page(site, 1_000_000)
    links = Links(text)
    sized = sorted((a for a in links.links if 'icon' in a['rel']),
                   key=lambda a: -max(map(int, re.findall(r'(\d+)x', a.get('sizes') or '')), default=0))
    for href in links.find('apple-touch-icon') + [a['href'] for a in sized] + ['/favicon.ico']:
        try:
            img = QImage.fromData(get(urljoin(base, href), 2_000_000)[1])
        except Exception:
            continue
        if img.width() >= 16:
            img.scaled(64, 64, Qt.KeepAspectRatio, Qt.SmoothTransformation).save(path, 'PNG')
            return path
    raise ValueError('no icon')


def read_opml(data):
    out = []

    def walk(el, folder):
        for o in el.findall('outline'):
            name = o.get('title') or o.get('text')
            if o.get('xmlUrl'):
                out.append((o.get('xmlUrl'), name, folder))
            else:
                walk(o, name)
    walk(ET.fromstring(data).find('body'), None)
    return out


def write_opml(feeds):
    root = ET.Element('opml', version='2.0')
    ET.SubElement(ET.SubElement(root, 'head'), 'title').text = 'Tidings'
    body = ET.SubElement(root, 'body')
    folders = {}
    for url, title, site, folder in feeds:
        if folder and folder not in folders:
            folders[folder] = ET.SubElement(body, 'outline', text=folder, title=folder)
        ET.SubElement(folders[folder] if folder else body, 'outline', dict(type='rss', text=title, title=title, xmlUrl=url, **({'htmlUrl': site} if site else {})))
    return ET.tostring(root, encoding='unicode', xml_declaration=True)


def discover(url):
    if '://' not in url:
        url = 'https://' + url
    d = feedparser.parse(url)
    if d.get('version'):
        return url, d
    base, text = page(url, 2_000_000)
    here = base.split('?')[0].rstrip('/')
    for guess in Links(text).find('alternate', r'(rss|atom)\+xml')[:1] or [
            here + '/.rss', here + '/releases.atom', '/feed', '/rss', '/feed.xml', '/atom.xml', '/rss.xml', '/index.xml']:
        url = urljoin(base, guess)
        d = feedparser.parse(url)
        if d.get('version'):
            return url, d
    raise ValueError('no feed found there')


def plain(s):
    return ' '.join(html.unescape(re.sub(r'(?s)<(script|style)\b.*?</\1>|<[^>]*(>|$)', ' ', s or '')).split())


def when(ts):
    s = time.time() - ts
    if s >= 86400:
        return QLocale().toString(QDateTime.fromSecsSinceEpoch(ts).time(), QLocale.ShortFormat)
    return f'{int(s // 3600)}h' if s >= 3600 else f'{int(s // 60)}m' if s >= 60 else 'now'


def day(ts):
    d = QDateTime.fromSecsSinceEpoch(ts).date()
    n = d.daysTo(QDate.currentDate())
    if n <= 1:
        return ('Today', 'Yesterday')[max(n, 0)]
    return (QLocale().standaloneDayName(d.dayOfWeek()) if n < 7 else QLocale().toString(d, QLocale.LongFormat)).capitalize()


NOTES = DBusAddress('/org/freedesktop/Notifications', 'org.freedesktop.Notifications', 'org.freedesktop.Notifications')
PORTAL = DBusAddress('/org/freedesktop/portal/desktop', 'org.freedesktop.portal.Desktop', 'org.freedesktop.portal.Settings')
FILES = DBusAddress(PORTAL.object_path, PORTAL.bus_name, 'org.freedesktop.portal.FileChooser')


HOST_RULE = re.compile(r'(@@)?\|\|([a-z0-9.-]+)\^(?:\$([a-z,-]+))?')
HOST_OPTS = {'third-party', '3p', 'script', 'image', 'xmlhttprequest', 'subdocument', 'ping', 'media', 'websocket', 'other'}
PATH_RULE = re.compile(r'/[\w./-]+\|?')
COSMETIC = re.compile(r'([a-z0-9.,-]*)##(.+)')
NAV_TRACKING = re.compile(r'utm_|fbclid=|gclid=|mc_[ce]id=')


def parse_lists(texts):
    hosts, allow, paths, css = {'content-settings.com'}, set(), [], {'': ''}  # ponytail: Ad-Shield anti-adblock config host, missing from lists
    for i, text in enumerate(texts):
        cookies = i == len(texts) - 1
        for line in text.splitlines():
            if (m := HOST_RULE.fullmatch(line)) and (not m[3] or set(m[3].split(',')) <= HOST_OPTS):
                (allow if m[1] else hosts).add(m[2])
            elif (m := COSMETIC.fullmatch(line)) and (cookies or not m[1]):
                for d in m[1].split(','):
                    css[d] = css.get(d, '') + m[2] + '{display:none!important}'
            elif cookies and PATH_RULE.fullmatch(line):
                paths.append(re.escape(line.rstrip('|')))
    return hosts, allow, re.compile('|'.join(paths)) if paths else None, css


class Blocker(QWebEngineUrlRequestInterceptor):
    hosts = allow = frozenset()
    paths = None

    def load(self, folder):
        texts = []
        for url in LISTS:
            path = os.path.join(folder, os.path.basename(url))
            try:
                if not os.path.exists(path) or time.time() - os.path.getmtime(path) > 4 * 86400:
                    data = get(url)[1]
                    with open(path, 'wb') as f:
                        f.write(data)
                with open(path, encoding='utf-8', errors='replace') as f:
                    texts.append(f.read())
            except OSError as e:
                print(f'blocklist {url}: {e}', file=sys.stderr)
                texts.append('')
        self.hosts, self.allow, self.paths, css = parse_lists(texts)
        return ('(() => { const m = %s, h = location.hostname.split("."), s = new CSSStyleSheet(); let c = m[""]'
                '; for (let i = 0; i < h.length - 1; i++) c += m[h.slice(i).join(".")] || ""'
                '; s.replaceSync(c); document.adoptedStyleSheets = [...document.adoptedStyleSheets, s] })()') % json.dumps(css)

    def interceptRequest(self, info):
        info.setHttpHeader(b'Sec-GPC', b'1')
        info.setHttpHeader(b'DNT', b'1')
        url = info.requestUrl()
        if info.resourceType() == QWebEngineUrlRequestInfo.ResourceType.ResourceTypeMainFrame:
            q = url.query(QUrl.FullyEncoded).split('&')
            if any(NAV_TRACKING.match(kv) for kv in q):
                url.setQuery('&'.join(kv for kv in q if not NAV_TRACKING.match(kv)) or None)
                info.redirect(url)
            return
        p = url.host().split('.')
        ds = {'.'.join(p[i:]) for i in range(len(p) - 1)}
        if not ds & self.allow and (ds & self.hosts or self.paths and self.paths.search(url.path(QUrl.FullyEncoded))):
            info.block(True)


class Rows(QAbstractListModel):
    def __init__(self, keys):
        super().__init__()
        self.keys, self.rows = keys, []

    def roleNames(self):
        return {Qt.UserRole + i: k.encode() for i, k in enumerate(self.keys)}

    def rowCount(self, parent=QModelIndex()):
        return len(self.rows)

    def data(self, index, role):
        return self.rows[index.row()][self.keys[role - Qt.UserRole]]

    def set(self, rows, reset=False):
        ids = [r['id'] for r in rows]
        old = [r['id'] for r in self.rows]
        if reset or [i for i in old if i in set(ids)] != [i for i in ids if i in set(old)]:
            self.beginResetModel()
            self.rows = rows
            return self.endResetModel()
        for i in reversed(range(len(old))):
            if old[i] not in ids:
                self.beginRemoveRows(QModelIndex(), i, i)
                del self.rows[i]
                self.endRemoveRows()
        for i, r in enumerate(rows):
            if i == len(self.rows) or self.rows[i]['id'] != r['id']:
                self.beginInsertRows(QModelIndex(), i, i)
                self.rows.insert(i, r)
                self.endInsertRows()
            elif self.rows[i] != r:
                self.rows[i] = r
                self.dataChanged.emit(self.index(i), self.index(i))

    def find(self, rid):
        return next((i for i, r in enumerate(self.rows) if r['id'] == rid), None)


class Backend(QObject):
    changed = Signal()
    mutesChanged = Signal()
    themeChanged = Signal()
    busyChanged = Signal()
    message = Signal(str, bool)
    added = Signal(str)
    activate = Signal(int, int)
    shown = Signal()
    articleReady = Signal(int)
    subscribe = Signal(str)
    _fetched = Signal(object, object)
    _discovered = Signal(object)
    _extracted = Signal(int, object)
    _iconed = Signal(int, object)
    _blocked = Signal(object)
    cosmeticChanged = Signal()
    _dbus = Signal(object)

    def __init__(self, db, cache):
        super().__init__()
        self.db, self.cache, self.pool, self.win = db, cache, ThreadPoolExecutor(8), None
        self.feeds = Rows(('id', 'title', 'unread', 'web', 'icon', 'error', 'notify', 'url', 'site', 'folder', 'full'))
        self.items = Rows(('id', 'feed_id', 'title', 'feed', 'preview', 'image', 'when', 'day', 'read', 'starred', 'link', 'web', 'media', 'hidden'))
        self._feed = self.current = 0
        self._folder, self._folders = '', {}
        self._unread, self._query = False, ''
        self._dark, self._accent, self.custom = False, QColor('#3584e4'), {}
        os.makedirs(os.path.dirname(THEME), exist_ok=True)
        # generators either replace the file (dir event) or rewrite it in place (file event)
        self.watcher = QFileSystemWatcher([os.path.dirname(THEME)], self, directoryChanged=self.load_theme, fileChanged=self.load_theme)
        self.load_theme()
        self.font = f'"{QGuiApplication.font().family()}"'
        self.pending, self.note, self.fresh, self.requests = {}, 0, False, {}
        self.inflight, self.extracting, self.iconing, self.undo_ids = set(), set(), set(), []
        self.asked, self.limit = 0, LIMIT
        self._hidden, self._calm, self._expire, self.hidden_n, self.rules = False, False, 0, 0, []
        db.create_function('muted', 3, self.muted)
        self.load_mutes(True)
        self.reload = QTimer(self, singleShot=True, interval=300, timeout=self.load_items)
        self._fetched.connect(self.fetched)
        self._discovered.connect(self.discovered)
        self._extracted.connect(self.extracted)
        self._iconed.connect(self.iconed)
        self._dbus.connect(self.on_dbus)
        self.blocker, self.cosmetic_js = Blocker(self), ''
        self._blocked.connect(self.blocked)
        self.pool.submit(self.blocker.load, os.path.dirname(cache)).add_done_callback(self._blocked.emit)
        self.bus = DBusRouter(open_dbus_connection())
        q = Queue()
        for rule in (MatchRule(type='signal', interface=NOTES.interface),
                     MatchRule(type='signal', interface=PORTAL.interface, member='SettingChanged'),
                     MatchRule(type='signal', interface='org.freedesktop.portal.Request', member='Response')):
            self.bus.send_and_get_reply(message_bus.AddMatch(rule))
            self.bus.filter(rule, queue=q)
        threading.Thread(target=self.pump, args=(q,), daemon=True).start()
        for key in ('color-scheme', 'accent-color'):
            try:
                self.setting(key, self.call(PORTAL, 'ReadOne', 'ss', ('org.freedesktop.appearance', key))[0][1])
            except Exception as e:
                print(f'portal {key}: {e}', file=sys.stderr)
        self.load_feeds()
        self.load_items(True)
        QTimer(self, timeout=self.refresh, interval=REFRESH_SECS * 1000).start()
        QTimer(self, timeout=self.load_items, interval=60 * 1000).start()

    def pump(self, q):
        while True:
            self._dbus.emit(q.get())

    def call(self, addr, method, sig, args):
        return unwrap_msg(self.bus.send_and_get_reply(new_method_call(addr, method, sig, args), timeout=5))

    def setting(self, key, v):
        if key == 'color-scheme':
            self._dark = v == 1
            QGuiApplication.styleHints().setColorScheme(Qt.ColorScheme.Dark if self._dark else Qt.ColorScheme.Light)
        elif key == 'accent-color' and all(0 <= c <= 1 for c in v):
            self._accent = QColor.fromRgbF(*v)
        self.themeChanged.emit()

    def load_theme(self):
        if os.path.exists(THEME) and THEME not in self.watcher.files():
            self.watcher.addPath(THEME)
        try:
            with open(THEME) as f:
                self.custom = json.load(f)
        except (OSError, ValueError):
            self.custom = {}
        self.themeChanged.emit()

    def palette(self):
        return {**PALETTE[self._dark], 'accent': self._accent.name(), 'opacity': 0.7, **self.custom}

    def muted(self, fid, title, content):
        return int(any((f is None or f == fid) and (rx.search(title or '') or not t and rx.search(content or ''))
                       for f, t, rx in self.rules))

    def load_mutes(self, force=False):
        if self.db.execute('delete from mutes where expires < ?', (time.time(),)).rowcount or force:
            rows = self.db.execute('select m.*, coalesce(f.name, f.title, f.url) as feed from mutes m left join feeds f on f.id = m.feed_id'
                                   ' order by m.id').fetchall()
            self._mutes = [dict(id=r['id'], text=r['text'], feed=r['feed'] or '', titleOnly=bool(r['title_only']),
                                expires=QLocale().toString(QDateTime.fromSecsSinceEpoch(r['expires']).date(), QLocale.ShortFormat)
                                if r['expires'] else '') for r in rows]
            self.rules = [(r['feed_id'], r['title_only'], re.compile(
                r'(?<!\w)(?:' + '|'.join(re.escape(w.strip()) for w in r['text'].split(',') if w.strip()) + r')(?!\w)', re.I))
                for r in rows]
            self.db.execute('update items set muted = muted(feed_id, title, content)')
            self.db.commit()
            self.mutesChanged.emit()
            return True

    @Slot(str, int, bool, int)
    def addMute(self, text, fid, title_only, days):
        if text.strip(' ,'):
            self.db.execute('insert into mutes(text, feed_id, title_only, expires) values(?, ?, ?, ?)',
                            (text.strip(), fid if fid > 0 else None, int(title_only), days and int(time.time()) + days * 86400 or None))
            self.refilter()

    @Slot(int)
    def removeMute(self, mid):
        self.db.execute('delete from mutes where id = ?', (mid,))
        self.refilter()

    def refilter(self):
        self.load_mutes(True)
        self.load_feeds()
        self.load_items()

    def expire_old(self):
        if self._expire:
            self.db.execute('update items set read = 1 where not read and not starred and published < ?', (time.time() - self._expire * 86400,))
            self.db.commit()

    def set_expire(self, days):
        self._expire = days
        self.expire_old()
        self.load_feeds()
        self.load_items()

    def set_hidden(self, on):
        self._hidden = on
        self.load_items()
        self.changed.emit()

    def set_calm(self, on):
        self._calm = on
        self.changed.emit()

    def on_dbus(self, m):
        name, args = m.header.fields[HeaderFields.member], m.body
        if name == 'SettingChanged':
            if args[0] == 'org.freedesktop.appearance':
                self.setting(args[1], args[2][1])
            return
        if name == 'Response':
            done = self.requests.pop(m.header.fields[HeaderFields.path].rsplit('/', 1)[-1], None)
            if done and args[0] == 0:
                done(unquote(urlparse(args[1]['uris'][1][0]).path))
            return
        if not self.note or args[0] != self.note:
            return
        if name == 'ActivationToken':
            os.environ['XDG_ACTIVATION_TOKEN'] = args[1]
        elif name in ('ActionInvoked', 'NotificationClosed'):
            pending, self.pending, self.note = self.pending, {}, 0
            ids = [i for v in pending.values() for i in v]
            if name == 'NotificationClosed':
                return
            if args[1] == 'read':
                self.db.executemany('update items set read = 1 where id = ?', [(i,) for i in ids])
                self.db.commit()
                self.load_feeds()
                self.load_items()
            else:
                self.activate.emit(next(iter(pending)) if len(pending) == 1 else 0, ids[0] if len(ids) == 1 else 0)

    dark = Property(bool, lambda s: s._dark, notify=themeChanged)
    theme = Property('QVariant', palette, notify=themeChanged)
    busy = Property(bool, lambda s: bool(s.inflight), notify=busyChanged)
    feedModel = Property(QObject, lambda s: s.feeds, constant=True)
    itemModel = Property(QObject, lambda s: s.items, constant=True)

    def set_feed(self, fid, folder=''):
        if fid > 0 and self.feeds.find(fid) is None:
            fid = 0
        folder = folder if fid == 0 and folder in self._folders else ''
        if (fid, folder) != (self._feed, self._folder):
            self._feed, self._folder = fid, folder
            self.load_items(True)
            self.changed.emit()
        self.withdraw(fid, folder)

    def set_unread(self, on):
        self._unread = on
        self.load_items()
        self.changed.emit()

    def set_query(self, q):
        if q != self._query:
            self._query = q
            self.load_items(True)
            self.changed.emit()

    feedId = Property(int, lambda s: s._feed, set_feed, notify=changed)
    unreadOnly = Property(bool, lambda s: s._unread, set_unread, notify=changed)
    query = Property(str, lambda s: s._query, set_query, notify=changed)
    folder = Property(str, lambda s: s._folder, lambda s, v: s.set_feed(0, v), notify=changed)
    folders = Property('QVariant', lambda s: s._folders, notify=changed)
    showHidden = Property(bool, lambda s: s._hidden, set_hidden, notify=changed)
    hiddenCount = Property(int, lambda s: s.hidden_n, notify=changed)
    calm = Property(bool, lambda s: s._calm, set_calm, notify=changed)
    expireDays = Property(int, lambda s: s._expire, set_expire, notify=changed)
    mutes = Property('QVariant', lambda s: s._mutes, notify=mutesChanged)
    feedTitle = Property(str, lambda s: s._folder or next((r['title'] for r in s.feeds.rows if r['id'] == s._feed), ''), notify=changed)

    def scope(self, fid, folder=''):
        if folder:
            return 'feed_id in (select id from feeds where folder = ?)', (folder,)
        return ('starred', ()) if fid == -1 else ('feed_id = ?', (fid,)) if fid else ('1', ())

    def load_feeds(self):
        counts = dict(self.db.execute('select feed_id, count(*) from items where not read and dup is null and not muted group by feed_id').fetchall())
        saved = self.db.execute('select count(*) from items where starred').fetchone()[0]
        blank = dict(web=0, icon='', error='', notify=1, url='', site='', folder='', full=0)
        self.feeds.set([dict(blank, id=0, title='All Articles', unread=sum(counts.values())),
                        dict(blank, id=-1, title='Saved', unread=saved)] + [
            dict(id=r['id'], title=r['name'] or r['title'] or r['url'], unread=counts.get(r['id'], 0), web=r['web'],
                 icon=r['icon'] or '', error=r['error'] or '', notify=r['notify'], url=r['url'], site=r['site'] or '',
                 folder=r['folder'] or '', full=r['fulltext'])
            for r in self.db.execute("select * from feeds order by coalesce(folder, '') collate nocase,"
                                     ' coalesce(name, title, url) collate nocase')])
        self._folders = {}
        for r in self.feeds.rows:
            if r['folder']:
                self._folders[r['folder']] = self._folders.get(r['folder'], 0) + r['unread']
        if self._folder and self._folder not in self._folders:
            self.set_feed(self._feed)
        self.changed.emit()

    def load_items(self, reset=False):
        if reset:
            self.limit = LIMIT
        w, args = self.scope(self._feed, self._folder)
        args = list(args)
        if self._unread:
            w, args = w + ' and (not read or i.id = ?)', args + [self.current]
        if self._query:
            w, args = w + ' and (i.title like ? or i.content like ? or i.full like ?)', args + [f'%{self._query}%'] * 3
        hide = '(i.dup is not null or i.muted)'
        n = 0 if self._feed == -1 else self.db.execute(f'select count(*) from items i where {w} and {hide}', args).fetchone()[0]
        if self._feed != -1 and not self._hidden:
            w += f' and not {hide}'
        q = ("select i.id, i.feed_id, i.title, i.link, i.published, i.image, i.read, i.starred, f.web, i.media_type,"
             f" {hide} as hidden, coalesce(f.name, f.title) as feed_title, substr(coalesce(nullif(i.full, ''), i.content), 1, 3000) as body"
             f" from items i join feeds f on f.id = i.feed_id where {w}")
        self.items.set([dict(id=r['id'], feed_id=r['feed_id'], title=plain(r['title']), feed=r['feed_title'] or '',
                             preview=plain(r['body'])[:300], image=r['image'] or '', when=when(r['published']),
                             day=day(r['published']), read=r['read'], starred=r['starred'], link=r['link'] or '', web=r['web'],
                             media=r['media_type'] or '', hidden=bool(r['hidden']))
                        for r in self.db.execute(q + ' order by published desc, i.id desc limit ?', args + [self.limit])], reset)
        if n != self.hidden_n:
            self.hidden_n = n
            self.changed.emit()

    def patch(self, iid, **kv):
        self.db.execute(f'update items set {", ".join(f"{k} = ?" for k in kv)} where id = ?', (*kv.values(), iid))
        self.db.commit()
        if kv.get('read'):
            self.forget([iid])
        i = self.items.find(iid)
        if i is not None:
            self.items.rows[i].update(kv)
            self.items.dataChanged.emit(self.items.index(i), self.items.index(i))
        self.load_feeds()
        self.reload.start()

    @Slot()
    def more(self):
        if len(self.items.rows) >= self.limit:
            self.limit += LIMIT
            self.load_items()

    @Slot(int, float)
    def setPos(self, iid, t):
        self.db.execute('update items set pos = ? where id = ?', (t, iid))
        self.db.commit()

    @Slot(int, int, result='QVariant')
    def neighbor(self, iid, step):
        i = self.items.find(iid)
        i = (-1 if step > 0 else len(self.items.rows)) if i is None else i
        if 0 <= i + step < len(self.items.rows):
            return dict(self.items.rows[i + step], index=i + step)

    @Slot(int, result=int)
    def feedStep(self, n):
        ids = [r['id'] for r in self.feeds.rows]
        return ids[((ids.index(self._feed) if not self._folder else 0) + n) % len(ids)]

    @Slot(int, result='QVariant')
    def nextUnread(self, iid):
        rows, i = self.items.rows, self.items.find(iid)
        i = -1 if i is None else i
        j = next((j for j in [*range(i + 1, len(rows)), *range(i)] if not rows[j]['read']), None)
        return None if j is None else dict(rows[j], index=j)

    @Slot(result=int)
    def nextFeed(self):
        rows = [r for r in self.feeds.rows if r['id'] > 0]
        i = next((i for i, r in enumerate(rows) if r['id'] == self._feed), -1)
        return next((r['id'] for r in rows[i + 1:] + rows[:i] if r['unread']), 0)

    @Slot(int, result=str)
    def article(self, iid):
        self.current = iid
        r = self.db.execute('select i.*, f.fulltext, coalesce(f.name, f.title) as feed_title from items i join feeds f on f.id = i.feed_id'
                            ' where i.id = ?', (iid,)).fetchone()
        if not r['read']:
            self.patch(iid, read=1)
        body, extra = r['content'], ''
        if r['full']:
            body, extra = r['full'], f'<div class="source">{r["content"]}</div>' if plain(r['content']) else ''
        elif r['full'] is None and r['link'] and r['media_type'] != 'youtube' and (r['fulltext'] or thin(r['content'])):
            self.fetch_full(iid, r['link'])
        if iid in self.extracting:
            extra = '<p class="meta loading">Fetching full article…</p>'
        e = html.escape
        if not re.search(r'</?[a-z][^>]*>', body):
            body = '<p style="white-space:pre-line">' + re.sub(r'https?://[^\s<]+', r'<a href="\g<0>">\g<0></a>', e(body)) + '</p>'
        body = re.sub(r'["“](<q\b[^>]*>)|(</q>)["”]', r'\1\2', body)
        mins = round(len(plain(body).split()) / 230)
        meta = ' · '.join(dict.fromkeys(filter(None, (r['feed_title'], r['author'],
                                        QLocale().toString(QDateTime.fromSecsSinceEpoch(r['published']).date(), QLocale.LongFormat),
                                        mins and not r['media'] and f'{mins} min read'))))
        t, src = r['media_type'], e(r['media'] or '')
        if r['pos'] and t != 'youtube':
            src += f'#t={int(r["pos"])}'
        player = (f'<iframe class="video" src="{src}" allowfullscreen></iframe>' if t == 'youtube'
                  else f'<{t} controls preload="metadata" src="{src}"></{t}>' if t else '')
        t = self.palette()
        return (f'<!doctype html><html class="{"dark" if self._dark else ""}" style="--accent:{t["accent"]};--fg:{t["fg"]};'
                f'--font:{e(self.font)}"><meta charset="utf-8"><script>history.scrollRestoration="manual"</script><style>{STYLE}</style>'
                f'<h1><a href="{e(r["link"] or "")}">{e(plain(r["title"]))}</a></h1><div class="meta">{e(meta)}</div>'
                f'{player}{body}{extra}')

    def fetch_full(self, iid, link):
        if iid not in self.extracting:
            self.extracting.add(iid)
            self.pool.submit(extract, link).add_done_callback(lambda fut: self._extracted.emit(iid, fut))

    @Slot(int)
    def toggleFull(self, iid):
        r = self.db.execute('select full, link from items where id = ?', (iid,)).fetchone()
        if r['full']:
            self.db.execute("update items set full = '' where id = ?", (iid,))
            self.db.commit()
            self.reload.start()
        elif r['link']:
            self.asked = iid
            self.fetch_full(iid, r['link'])
        self.articleReady.emit(iid)

    def prefetch(self):
        rows = self.db.execute('select i.id, link, content, fulltext from items i join feeds f on f.id = feed_id where not read'
                               " and full is null and link is not null and media_type is not 'youtube'"
                               ' order by published desc limit 100').fetchall()
        for r in [r for r in rows if r['fulltext'] or thin(r['content'])][:20]:
            self.fetch_full(r['id'], r['link'])

    def extracted(self, iid, fut):
        self.extracting.discard(iid)
        try:
            body, og = fut.result()
        except Exception as e:
            body, og = '', None
            print(f'extract {iid}: {e}', file=sys.stderr)
        if iid == self.asked:
            self.asked = 0
            if not body:
                self.message.emit("Couldn't find the full article", False)
        self.db.execute('update items set full = ?, image = coalesce(image, ?) where id = ?', (body, og, iid))
        self.db.commit()
        self.reload.start()
        if iid == self.current:
            self.articleReady.emit(iid)

    def fetch_icon(self, fid, site):
        if site and fid not in self.iconing:
            self.iconing.add(fid)
            self.pool.submit(favicon, site, os.path.join(self.cache, f'{fid}.png')).add_done_callback(
                lambda fut: self._iconed.emit(fid, fut))

    def iconed(self, fid, fut):
        try:
            self.db.execute('update feeds set icon = ? where id = ?', (fut.result(), fid))
            self.db.commit()
            self.load_feeds()
        except Exception as e:
            print(f'icon {fid}: {e}', file=sys.stderr)

    def blocked(self, fut):
        try:
            self.cosmetic_js = fut.result()
            self.cosmeticChanged.emit()
        except Exception as e:
            print(f'blocklists: {e}', file=sys.stderr)

    cosmetic = Property(str, lambda s: s.cosmetic_js, notify=cosmeticChanged)

    @Slot(int, bool)
    def setRead(self, iid, on):
        self.patch(iid, read=int(on))

    @Slot(int, bool)
    def setStarred(self, iid, on):
        self.patch(iid, starred=int(on))

    def set_feed_col(self, fid, col, v):
        self.db.execute(f'update feeds set {col} = ? where id = ?', (v, fid))
        self.db.commit()
        self.load_feeds()
        self.load_items()

    @Slot(int, bool)
    def setWeb(self, fid, on):
        self.set_feed_col(fid, 'web', int(on))

    @Slot(bool)
    def setSerif(self, on):
        self.font = 'serif' if on else f'"{QGuiApplication.font().family()}"'
        self.articleReady.emit(self.current)

    @Slot(int, bool)
    def setFull(self, fid, on):
        self.set_feed_col(fid, 'fulltext', int(on))

    @Slot(int, bool)
    def setNotify(self, fid, on):
        self.set_feed_col(fid, 'notify', int(on))

    @Slot(int, str)
    def renameFeed(self, fid, name):
        self.set_feed_col(fid, 'name', name.strip() or None)

    @Slot(int, str)
    def setFolder(self, fid, name):
        self.set_feed_col(fid, 'folder', name.strip() or None)

    @Slot(str, str)
    def renameFolder(self, old, new):
        self.db.execute('update feeds set folder = ? where folder = ?', (new.strip() or None, old))
        self.db.commit()
        if self._folder == old:
            self._folder = new.strip()
        self.load_feeds()
        self.load_items()

    @Slot()
    def refresh(self):
        for f in self.db.execute('select * from feeds').fetchall():
            if f['id'] not in self.inflight:
                self.inflight.add(f['id'])
                self.pool.submit(feedparser.parse, f['url'], etag=f['etag'], modified=f['modified']).add_done_callback(
                    lambda fut, f=dict(f): self._fetched.emit(f, fut))
        self.busyChanged.emit()

    def fetched(self, f, fut):
        self.inflight.discard(f['id'])
        new, err = [], None
        try:
            d = fut.result()
            new = store(self.db, f['id'], d)
            if not d.get('version') and d.get('status') != 304:
                err = str(d.get('bozo_exception') or f"HTTP {d.get('status')}")
            elif not f['icon']:
                self.fetch_icon(f['id'], d.feed.get('link') or f['site'] or urljoin(f['url'], '/'))
        except Exception as e:
            err = str(e)
        if err != f['error']:
            self.db.execute('update feeds set error = ? where id = ?', (err, f['id']))
            self.db.commit()
        if new and f['notify'] and f['fetched'] and not self.win.isActive():
            self.pending.setdefault(f['id'], [])[:0] = new
            self.fresh = True
        if not self.inflight:
            self.load_mutes()
            self.expire_old()
            self.db.execute('delete from items where read and not starred and published < ?', (time.time() - KEEP_DAYS * 86400,))
            self.db.commit()
            self.busyChanged.emit()
            self.prefetch()
            self.load_feeds()
            self.load_items()
            if self.fresh:
                self.fresh = False
                self.notify()

    def notify(self):
        ids = [i for v in self.pending.values() for i in v]
        rows = self.db.execute(f'select i.title, coalesce(f.name, f.title, f.url) from items i join feeds f on f.id = i.feed_id'
                               f' where i.dup is null and not i.muted and i.id in ({",".join("?" * len(ids))}) order by i.published desc', ids).fetchall()
        if rows:
            head, body = summary([tuple(r) for r in rows])
            self.note = self.call(NOTES, 'Notify', 'susssasa{sv}i', (
                'Tidings', self.note, 'application-rss+xml', head, html.escape(body, quote=False),
                ['default', 'Open', 'read', 'Mark as Read'], {'desktop-entry': ('s', APP_ID)}, -1))[0]

    def withdraw(self, fid, folder=''):
        gone = [r['id'] for r in self.feeds.rows if r['folder'] == folder] if folder else list(self.pending) if fid == 0 else [fid]
        self.forget(i for f in gone for i in self.pending.get(f, []))

    def forget(self, ids):
        ids, n = set(ids), sum(map(len, self.pending.values()))
        self.pending = {f: k for f, v in self.pending.items() if (k := [i for i in v if i not in ids])}
        if not self.note or n == sum(map(len, self.pending.values())):
            return
        if self.pending:
            self.notify()
        else:
            self.call(NOTES, 'CloseNotification', 'u', (self.note,))
            self.note = 0

    @Slot(str)
    def addFeed(self, url):
        self.pool.submit(discover, url.strip()).add_done_callback(self._discovered.emit)

    def discovered(self, fut):
        try:
            url, d = fut.result()
        except Exception as e:
            return self.added.emit(str(e) if isinstance(e, ValueError) else f"Couldn't load that address ({e})")
        try:
            fid = self.db.execute('insert into feeds(url) values(?)', (url,)).lastrowid
        except sqlite3.IntegrityError:
            return self.added.emit("You're already subscribed to that feed")
        store(self.db, fid, d)
        self.prefetch()
        self.fetch_icon(fid, d.feed.get('link') or urljoin(url, '/'))
        self.added.emit('')
        self.load_feeds()
        self.set_feed(fid)

    @Slot(int)
    def removeFeed(self, fid):
        self.withdraw(fid)
        self.db.execute('delete from feeds where id = ?', (fid,))
        self.db.commit()
        self.load_feeds()
        self.set_feed(0) if self._feed == fid else self.load_items()

    @Slot(int, str)
    @Slot(int, str, int)
    def markAllRead(self, fid, folder, older_than=0):
        w, args = self.scope(fid, folder)
        if older_than:
            w, args = w + ' and published < (select published from items where id = ?)', (*args, older_than)
        self.undo_ids = [r[0] for r in self.db.execute(f'select id from items where not read and {w}', args)]
        if not self.undo_ids:
            return
        self.db.executemany('update items set read = 1 where id = ?', [(i,) for i in self.undo_ids])
        self.db.commit()
        self.forget(self.undo_ids)
        self.load_feeds()
        self.load_items()
        n = len(self.undo_ids)
        self.message.emit(f'Marked {n} article{"s" * (n != 1)} as read', True)

    @Slot()
    def undo(self):
        self.db.executemany('update items set read = 0 where id = ?', [(i,) for i in self.undo_ids])
        self.db.commit()
        self.undo_ids = []
        self.load_feeds()
        self.load_items()

    def set_autostart(self, on):
        subprocess.run(['systemctl', '--user', 'enable' if on else 'disable', 'tidings.service'], check=False)
        self.changed.emit()

    autostart = Property(bool, lambda s: os.path.lexists(WANTS), set_autostart, notify=changed)

    @Slot(result=str)
    def clipboardUrl(self):
        t = QGuiApplication.clipboard().text().strip()
        return t if re.fullmatch(r'https?://\S+', t) else ''

    @Slot(str)
    @Slot(str, str)
    def copy(self, text, what='Link'):
        QGuiApplication.clipboard().setText(text)
        self.message.emit(f'{what} copied', False)

    def choose(self, method, title, opts, done):
        token = 'tidings' + secrets.token_hex(4)
        try:
            self.call(FILES, method, 'ssa{sv}', ('', title, dict(opts, handle_token=('s', token))))
            self.requests[token] = done
        except Exception as e:
            self.message.emit(f'No file chooser available ({e})', False)

    @Slot()
    def importOpml(self):
        self.choose('OpenFile', 'Import Subscriptions',
                    {'filters': ('a(sa(us))', [('OPML', [(0, '*.opml'), (0, '*.xml')])])}, self.importFile)

    @Slot(str)
    def importFile(self, path):
        try:
            with open(path, 'rb') as f:
                feeds = read_opml(f.read())
        except Exception:
            return self.message.emit(f"Couldn't read {os.path.basename(path)}", False)
        n = sum(self.db.execute('insert or ignore into feeds(url, title, folder) values(?, ?, ?)', f).rowcount for f in feeds)
        self.db.commit()
        self.message.emit(f'Added {n} feed{"s" * (n != 1)}' if n else 'Already subscribed to all of those', False)
        self.load_feeds()
        self.refresh()

    @Slot()
    def exportOpml(self):
        def save(path):
            with open(path, 'w') as f:
                f.write(write_opml(self.db.execute("select url, coalesce(name, title, url), site, folder from feeds order by coalesce(folder, '')").fetchall()))
            self.message.emit(f'Saved {os.path.basename(path)}', False)
        self.choose('SaveFile', 'Export Subscriptions', {'current_name': ('s', 'Subscriptions.opml')}, save)


def main():
    sock = os.path.join(os.environ.get('XDG_RUNTIME_DIR', '/tmp'), APP_ID)
    try:
        with blocking.open_dbus_connection() as c:
            if unwrap_msg(c.send_and_get_reply(new_method_call(PORTAL, 'ReadOne', 'ss', ('org.freedesktop.appearance', 'color-scheme')), timeout=2))[0][1] == 1:
                # Chromium ignores Qt's color scheme; only read at startup
                os.environ['QTWEBENGINE_CHROMIUM_FLAGS'] = os.environ.get('QTWEBENGINE_CHROMIUM_FLAGS', '') + ' --blink-settings=preferredColorScheme=0'
    except Exception as e:
        print(f'portal color-scheme: {e}', file=sys.stderr)
    QtWebEngineQuick.initialize()
    app = QApplication(sys.argv)
    peer = QLocalSocket()
    peer.connectToServer(sock)
    url = re.sub(r'^feed:(//)?', '', next((a for a in sys.argv[1:] if not a.startswith('-')), ''))
    if peer.waitForConnected(500):
        peer.write(url.encode() + b'\n')
        peer.waitForBytesWritten(500)
        return
    app.setApplicationName('Tidings')
    app.setOrganizationName('tidings')
    app.setDesktopFileName(APP_ID)
    app.setQuitOnLastWindowClosed(False)
    if not QIcon.themeName():
        QIcon.setThemeName('breeze')
    xdg = lambda var, default: os.path.join(os.environ.get(var) or os.path.expanduser(default), 'tidings')
    data, cache = xdg('XDG_DATA_HOME', '~/.local/share'), os.path.join(xdg('XDG_CACHE_HOME', '~/.cache'), 'icons')
    os.makedirs(data, exist_ok=True)
    os.makedirs(cache, exist_ok=True)
    backend = Backend(open_db(os.path.join(data, 'tidings.db')), cache)
    QQuickWindow.setDefaultAlphaBuffer(True)
    profile = QQuickWebEngineProfile.defaultProfile()
    profile.setUrlRequestInterceptor(backend.blocker)
    profile.cookieStore().setCookieFilter(lambda r: not r.thirdParty)
    rfd, wfd = socket.socketpair()
    wfd.setblocking(False)
    signal.set_wakeup_fd(wfd.fileno())
    signal.signal(signal.SIGTERM, lambda *_: app.quit())
    wake = QSocketNotifier(rfd.fileno(), QSocketNotifier.Read, app, activated=lambda: rfd.recv(64))
    engine = QQmlApplicationEngine()
    engine.setInitialProperties({'app': backend, 'visible': '--background' not in sys.argv})
    engine.load(QUrl.fromLocalFile(os.path.join(os.path.dirname(os.path.realpath(__file__)), 'Main.qml')))
    if not engine.rootObjects():
        return 1
    backend.win = engine.rootObjects()[0]
    QLocalServer.removeServer(sock)
    server = QLocalServer()

    def hello():
        c = server.nextPendingConnection()
        c.waitForReadyRead(500)
        u = bytes(c.readAll()).decode().strip()
        backend.subscribe.emit(u) if u else backend.shown.emit()
    server.newConnection.connect(hello)
    if url:
        QTimer.singleShot(0, lambda: backend.subscribe.emit(url))
    server.listen(sock)
    if QSystemTrayIcon.isSystemTrayAvailable():
        tray = QSystemTrayIcon(QIcon.fromTheme('application-rss+xml'), app)
        menu = QMenu()
        menu.addAction('Open Tidings', backend.shown.emit)
        menu.addAction('Refresh', backend.refresh)
        menu.addSeparator()
        menu.addAction('Quit', app.quit)
        tray.setContextMenu(menu)
        tray.activated.connect(lambda why: why == QSystemTrayIcon.Trigger and backend.shown.emit())
        backend.changed.connect(lambda: tray.setToolTip('Tidings' if backend.calm else f"Tidings — {backend.feeds.rows[0]['unread']} unread"))
        backend.changed.emit()
        tray.show()
    QTimer.singleShot(1000, backend.refresh)
    return app.exec()


if __name__ == '__main__':
    sys.exit(main())
