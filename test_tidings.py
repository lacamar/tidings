import feedparser
import tidings

RSS = '''<?xml version="1.0"?><rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel><title>T</title>
<item><guid>a</guid><title>One</title><link>https://x.org/1</link><description>&lt;p&gt;Hi &lt;img src="/i.png"&gt;&lt;/p&gt;</description></item>
<item><guid>b</guid><title>Two</title><link>https://x.org/2</link><media:thumbnail url="https://x.org/t.jpg"/></item>
</channel></rss>'''

db = tidings.open_db(':memory:')
fid = db.execute("insert into feeds(url) values('u')").lastrowid
d = feedparser.parse(RSS)
assert len(tidings.store(db, fid, d)) == 2
assert tidings.store(db, fid, d) == []
assert db.execute('select title from feeds').fetchone()[0] == 'T'
imgs = dict(db.execute('select guid, image from items').fetchall())
assert imgs == {'a': 'https://x.org/i.png', 'b': 'https://x.org/t.jpg'}, imgs
assert tidings.plain('<p>a<script>x</script> &amp; <b>b</b></p>') == 'a & b'

old = RSS.replace('<guid>b</guid>', '<guid>c</guid><pubDate>Mon, 01 Jan 2001 00:00:00 GMT</pubDate>')
assert tidings.store(db, fid, feedparser.parse(old)) == []
assert tidings.plain('a <img src="x') == 'a'
assert tidings.thin('<p>short</p>') and not tidings.thin('<img src=x>')

links = tidings.Links('<head><link rel="alternate" type="application/atom+xml" href="/feed"><link rel="Icon" href="/i.png"></head>')
assert links.find('alternate', r'(rss|atom)\+xml') == ['/feed'] and links.find('icon') == ['/i.png']
feeds = [('https://a.org/rss', 'A & B', 'https://a.org', None), ('https://c.org/atom', 'C', None, 'Tech'), ('https://d.org/', 'D', None, 'Tech')]
assert tidings.read_opml(tidings.write_opml(feeds)) == [(u, t, f) for u, t, _, f in feeds]
backlog = RSS.replace('</channel>', ''.join(f'<item><guid>{i}</guid><pubDate>Mon, 01 Jan 2001 00:00:00 GMT</pubDate></item>' for i in range(15)) + '</channel>')
fid2 = db.execute("insert into feeds(url) values('v')").lastrowid
assert len(tidings.store(db, fid2, feedparser.parse(backlog))) == 10
yt = feedparser.parse('<feed xmlns="http://www.w3.org/2005/Atom" xmlns:yt="http://www.youtube.com/xml/schemas/2015"><entry><yt:videoId>abc</yt:videoId></entry></feed>')
assert tidings.media_of(yt.entries[0]) == ('https://www.youtube-nocookie.com/embed/abc', 'youtube')
pod = feedparser.parse('<rss><channel><item><enclosure url="https://x.org/a.mp3" type="audio/mpeg" length="1"/></item></channel></rss>')
assert tidings.media_of(pod.entries[0]) == ('https://x.org/a.mp3', 'audio')
split = '<html><body>' + ''.join(f'<div><div class="post"><p>part{i} ' + 'word ' * 80 + '</p></div><p>ad</p></div>' for i in range(3)) + '</body></html>'
tidings.page = lambda url, n: (url, split)
body = tidings.extract('https://x.org/a')[0]
assert all(f'part{i}' in body for i in range(3)) and 'ad' not in tidings.plain(body).split(), body[:200]
tidings.page = lambda url, n: (url, '<html><body><p>nothing</p></body></html>')
assert tidings.extract('https://x.org/a')[0] == ''
assert tidings.summary([('<b>A</b>', 'F')]) == ('F', 'A')
assert tidings.summary([('A', 'F'), ('B', 'F')]) == ('2 new from F', 'A\nB')
assert tidings.summary([('A', 'F'), ('B', 'G'), ('C', 'G'), ('D', 'G')]) == ('4 new articles', 'F: A\nG: B\nG: C')
print('ok')
