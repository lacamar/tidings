import feedparser
from tidings import open_db, store


def feed(*entries):
    items = ''.join(f'<item><guid>{g}</guid><title>{t}</title><link>{l}</link></item>' for g, t, l in entries)
    return feedparser.parse(f'<rss version="2.0"><channel><title>x</title>{items}</channel></rss>')


db = open_db(':memory:')
db.create_function('muted', 3, lambda f, t, c: int('giveaway' in t.lower()))
a, b = (db.execute('insert into feeds(url, fetched) values(?, 1)', (u,)).lastrowid for u in 'ab')
store(db, a, feed(('1', 'Big news about the thing', 'https://www.ex.com/news/1?utm_source=a'), ('2', 'Weekly giveaway', 'https://ex.com/g')))
store(db, b, feed(('9', 'Other title', 'https://ex.com/news/1/'), ('8', 'big news about the thing', 'https://other.org/x'), ('7', 'Fresh', 'https://ex.com/f')))
got = {r[0]: (r[1] is not None, r[2]) for r in db.execute('select title, dup, muted from items')}
assert got == {'Big news about the thing': (False, 0), 'Weekly giveaway': (False, 1), 'Other title': (True, 0),
               'big news about the thing': (True, 0), 'Fresh': (False, 0)}, got
print('ok')
