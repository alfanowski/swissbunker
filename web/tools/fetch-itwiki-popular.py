#!/usr/bin/env python3
"""Fetch the most-viewed Italian Wikipedia articles, in view order.

WHY
---
Phase 1 measured that scaling to a full Italian Wikipedia would push roughly half of ordinary
queries past the rank cutoff, so the unranked fallback is the NORMAL path, not the exception.
FTS5 returns unranked matches in rowid order, which means insertion order decides what people
see — and insertion order is the Forge's to choose.

The claim to test is that inserting articles most-important-first turns "storage order" into
"importance order", at zero query-time cost. Testing it needs a real importance signal, and
pageviews are the cleanest one available without a full dump: an article people actually read
is an article worth showing first.

Uses the Wikimedia REST pageviews API for the ranking, then the MediaWiki API for the leads.
"""
import argparse
import json
import sqlite3
import sys
import time
import urllib.parse
import urllib.request
from typing import List, Tuple

REST = 'https://wikimedia.org/api/rest_v1/metrics/pageviews/top/it.wikipedia/all-access'
API = 'https://it.wikipedia.org/w/api.php'
USER_AGENT = 'SwissBunker/0.1 (https://github.com/alfanowski/swissbunker) python-urllib'

# Namespace and utility pages are not articles; they would pollute an importance ranking.
SKIP_PREFIXES = ('Speciale:', 'Discussione:', 'Wikipedia:', 'Aiuto:', 'Categoria:', 'Portale:')
SKIP_EXACT = {'Pagina_principale', 'Pagina principale'}


def get(url: str) -> dict:
    req = urllib.request.Request(url, headers={'User-Agent': USER_AGENT})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read().decode('utf-8'))


def top_titles(date: str, limit: int) -> List[Tuple[str, int]]:
    data = get(f'{REST}/{date}')
    out = []
    for a in data.get('items', [{}])[0].get('articles', []):
        title = a['article'].replace('_', ' ')
        if a['article'] in SKIP_EXACT or title in SKIP_EXACT:
            continue
        if any(a['article'].startswith(p) for p in SKIP_PREFIXES):
            continue
        out.append((title, a['views']))
        if len(out) >= limit:
            break
    return out


def leads(titles: List[str]) -> dict:
    """Lead sections for up to 20 titles. Whole-article extracts cap at one per request."""
    params = {
        'action': 'query', 'format': 'json', 'formatversion': '2',
        'titles': '|'.join(titles),
        'prop': 'extracts', 'explaintext': '1', 'exintro': '1', 'exlimit': '20',
        'redirects': '1',
    }
    data = get(API + '?' + urllib.parse.urlencode(params))
    return {p['title']: p.get('extract', '') for p in data.get('query', {}).get('pages', [])}


def main(date: str, count: int, out_path: str) -> None:
    ranked = top_titles(date, count)
    print(f'{len(ranked)} ranked titles from {date}')

    con = sqlite3.connect(out_path)
    con.execute('PRAGMA journal_mode=OFF')
    con.execute('PRAGMA page_size=4096')
    # views is UNINDEXED: it is carried for inspection, never searched. What actually orders
    # results is the INSERTION order below.
    con.execute('CREATE VIRTUAL TABLE docs USING fts5(title, body, views UNINDEXED)')

    written = 0
    for i in range(0, len(ranked), 20):
        chunk = ranked[i:i + 20]
        try:
            extracts = leads([t for t, _ in chunk])
        except Exception as err:                       # noqa: BLE001
            print(f'  batch failed ({err}), skipping', flush=True)
            continue
        # Insert in view order, highest first: this is the whole point of the exercise.
        batch = [(t, extracts.get(t, ''), v) for t, v in chunk if len(extracts.get(t, '')) > 150]
        if batch:
            con.executemany('INSERT INTO docs(title, body, views) VALUES (?,?,?)', batch)
            written += len(batch)
            con.commit()
            print(f'  {written} articles', flush=True)
        time.sleep(0.1)

    con.execute("INSERT INTO docs(docs) VALUES('optimize')")
    con.commit()
    con.close()
    print(f'built {out_path} — {written} articles, most-viewed first')


if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    # A fixed past date keeps the fixture reproducible; "yesterday" would drift.
    ap.add_argument('--date', default='2026/07/15')
    ap.add_argument('--count', type=int, default=400)
    ap.add_argument('--out', default='/Volumes/SWISSTEST/itwiki-popular.sqlite')
    args = ap.parse_args()
    if args.count > 1000:
        print('refusing: this tool is for a sample, not a mirror', file=sys.stderr)
        sys.exit(1)
    main(args.date, args.count, args.out)
