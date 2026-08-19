#!/usr/bin/env python3
"""Fetch a sample of real Italian Wikipedia articles and build an FTS5 index from them.

WHY THIS EXISTS, AND WHY IT IS SMALL
------------------------------------
Phase 1 was verified on a synthetic corpus: two million documents built from a random
vocabulary, so every term has roughly the SAME frequency. Natural language does not work
that way — it is zipfian, with a handful of words in most documents and a very long tail of
rare ones. The rank cutoff (skip ranking above N matches) was tuned against the wrong
distribution, and tuning against the wrong distribution is worse than not tuning at all.

A few thousand real articles are enough to measure the SHAPE of that distribution. A full
dump is 15+ GB and would tell us the same thing about the shape while costing three orders
of magnitude more disk.

Uses the MediaWiki API with plain-text extracts, so there is no wikitext to parse.

LEAD SECTIONS, NOT WHOLE ARTICLES
---------------------------------
The API caps whole-article extracts at ONE page per request — six thousand articles would be
six thousand round trips. Lead sections allow twenty per request, so the same sample costs
three hundred.

This is not a compromise: spec §6.2 indexes exactly the lead section in the dense branch,
because the incipit is what identifies an article. Measuring term distribution on leads is
therefore closer to the real product than measuring it on full text would have been.
"""
import argparse
import json
import sqlite3
import sys
import time
import urllib.parse
import urllib.request
from typing import Dict, List, Optional, Set, Tuple

API = 'https://it.wikipedia.org/w/api.php'
# The API policy asks for a descriptive agent identifying the project. No personal contact
# details: this is a third-party service and the project URL is the appropriate identifier.
USER_AGENT = 'SwissBunker/0.1 (https://github.com/alfanowski/swissbunker) python-urllib'


def api_get(params: dict) -> dict:
    params = {**params, 'format': 'json', 'formatversion': '2'}
    url = API + '?' + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers={'User-Agent': USER_AGENT})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read().decode('utf-8'))


def fetch_batch(continue_token: Optional[str]) -> Tuple[List[Tuple[str, str]], Optional[str]]:
    """One batch of random articles with plain-text extracts."""
    params = {
        'action': 'query',
        'generator': 'random',
        'grnnamespace': '0',      # articles only, no talk or template pages
        'grnlimit': '20',         # extracts caps at 20 pages per request
        'prop': 'extracts',
        'explaintext': '1',
        'exintro': '1',       # lead section only
        'exlimit': '20',      # only allowed with exintro; whole articles cap at 1 per request
    }
    if continue_token:
        params['grncontinue'] = continue_token
    data = api_get(params)
    out: List[Tuple[str, str]] = []
    for page in data.get('query', {}).get('pages', []):
        text = page.get('extract', '')
        # Stubs carry almost no lexical signal and would skew the frequency measurement.
        if len(text) > 250:
            out.append((page['title'], text))
    return out, data.get('continue', {}).get('grncontinue')


def build(target: int, out_path: str) -> None:
    con = sqlite3.connect(out_path)
    con.execute('PRAGMA journal_mode=OFF')
    con.execute('PRAGMA synchronous=OFF')
    con.execute('PRAGMA page_size=4096')
    con.execute('CREATE VIRTUAL TABLE docs USING fts5(title, body)')

    seen: Set[str] = set()
    token = None
    written = 0
    batch: List[Tuple[str, str]] = []

    while written < target:
        try:
            rows, token = fetch_batch(token)
        except Exception as err:                      # noqa: BLE001 - any network fault
            # A transient failure must not lose the articles already collected.
            print(f'  request failed ({err}), retrying in 3s', flush=True)
            time.sleep(3)
            continue

        for title, text in rows:
            if title in seen:
                continue                              # random generator repeats
            seen.add(title)
            batch.append((title, text))

        if len(batch) >= 200:
            con.executemany('INSERT INTO docs(title, body) VALUES (?, ?)', batch)
            written += len(batch)
            batch.clear()
            con.commit()
            print(f'  {written:,} / {target:,} articles', flush=True)

        time.sleep(0.1)                               # be a polite API client

    if batch:
        con.executemany('INSERT INTO docs(title, body) VALUES (?, ?)', batch)
        written += len(batch)
    con.commit()
    con.execute("INSERT INTO docs(docs) VALUES('optimize')")
    con.commit()
    con.close()

    import pathlib
    size = pathlib.Path(out_path).stat().st_size
    print(f'built {out_path} — {written:,} articles, {size / 1e6:.1f} MB')


if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('--count', type=int, default=6000)
    ap.add_argument('--out', default='/Volumes/SWISSTEST/itwiki-sample.sqlite')
    args = ap.parse_args()
    if args.count > 50000:
        print('refusing: this tool is for a sample, not a mirror', file=sys.stderr)
        sys.exit(1)
    build(args.count, args.out)
