#!/usr/bin/env python3
"""Measure how term frequency is actually distributed in real Italian text.

The rank cutoff was tuned on a synthetic corpus where every term appears about equally
often. Real language is zipfian. This measures the real shape, and — more importantly —
checks whether an ABSOLUTE cutoff (2000 matches) can be right at all, given that the same
word covers a fixed FRACTION of any corpus while the absolute count scales with its size.

Usage: analyse-term-distribution.py <index.sqlite>
"""
import sqlite3
import sys
import pathlib

# Queries a person might actually type into a bunker, not tokens drawn from the corpus.
# Split by expected shape so the answer is not an average over incomparable cases.
GOLDEN = {
    'common single words': [
        'acqua', 'storia', 'guerra', 'città', 'anno', 'famiglia', 'lavoro', 'scuola',
    ],
    'topic words': [
        'fotosintesi', 'penicillina', 'terremoto', 'vulcano', 'antibiotico', 'batteria',
    ],
    'rare / technical': [
        'clorofilliana', 'idroelettrica', 'poliomielite', 'sismografo',
    ],
    'multi-word questions': [
        'come depurare acqua', 'battaglia di Lepanto', 'come si potano gli ulivi',
        'sintomi della polmonite',
    ],
}


def sanitise(raw: str) -> str:
    """Mirror of sanitiseQuery in src/search/fts.ts — literal terms, quoted."""
    tokens = []
    for tok in raw.split():
        cleaned = ''.join(ch for ch in tok if ch.isalnum() or ch == '_')
        if cleaned:
            tokens.append(f'"{cleaned}"')
    return ' '.join(tokens) if tokens else '""'


def main(path: str) -> None:
    con = sqlite3.connect(f'file:{path}?mode=ro', uri=True)
    total_docs = con.execute('SELECT count(*) FROM docs').fetchone()[0]
    size_mb = pathlib.Path(path).stat().st_size / 1e6
    print(f'corpus: {total_docs:,} documents, {size_mb:.1f} MB\n')

    print(f"{'query':<28} {'matches':>9} {'% of corpus':>12}  group")
    print('-' * 70)
    rows = []
    for group, queries in GOLDEN.items():
        for q in queries:
            n = con.execute('SELECT count(*) FROM docs WHERE docs MATCH ?',
                            (sanitise(q),)).fetchone()[0]
            pct = 100.0 * n / total_docs
            rows.append((q, n, pct, group))
            print(f'{q:<28} {n:>9,} {pct:>11.2f}%  {group}')

    # The decisive question: a cutoff expressed in absolute matches cannot mean the same
    # thing on a 6k-document sample as on 1.8M articles, because the same word keeps its
    # share while its count grows a hundredfold.
    print('\n' + '=' * 70)
    print('If the cutoff were an absolute 2000 matches:')
    over = [r for r in rows if r[1] > 2000]
    print(f'  on THIS corpus ({total_docs:,} docs): {len(over)}/{len(rows)} queries unranked')
    for q, n, pct, _ in sorted(over, key=lambda r: -r[1])[:6]:
        print(f'      {q:<26} {n:>8,} ({pct:.1f}%)')

    print('\nScaled to a full Italian Wikipedia (~1,900,000 articles), assuming each query')
    print('keeps the same share of the corpus:')
    scaled = [(q, int(pct / 100 * 1_900_000), pct) for q, n, pct, _ in rows]
    over_scaled = [s for s in scaled if s[1] > 2000]
    print(f'  {len(over_scaled)}/{len(scaled)} queries would exceed 2000 matches')
    for q, n, pct in sorted(over_scaled, key=lambda r: -r[1])[:8]:
        print(f'      {q:<26} {n:>9,} ({pct:.1f}%)')

    con.close()


if __name__ == '__main__':
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    main(sys.argv[1])
