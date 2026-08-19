#!/usr/bin/env python3
"""Build the small FTS5 database the unit tests run against.

Deliberately tiny — a few hundred KB — so it stays in the repo's reach and the unit suite
stays fast. The multi-gigabyte case belongs to the conformance suite, not here.

The needle is planted at a known row so the expected answer is known in advance rather than
merely plausible: the discipline that caught the Phase 0 false positives.
"""
import sqlite3
import pathlib
import random
import string

out = pathlib.Path(__file__).resolve().parent.parent / "test" / "fixtures" / "tiny-fts.sqlite"
out.parent.mkdir(parents=True, exist_ok=True)
out.unlink(missing_ok=True)

con = sqlite3.connect(out)
con.execute("PRAGMA journal_mode=OFF")
con.execute("PRAGMA page_size=4096")
con.execute("CREATE VIRTUAL TABLE docs USING fts5(title, body)")

random.seed(1337)  # reproducible fixture: a flaky corpus makes a flaky test suite
words = [''.join(random.choices(string.ascii_lowercase, k=6)) for _ in range(500)]
rows = []
for i in range(2000):
    body = ' '.join(random.choices(words, k=60))
    if i == 1337:
        body += ' xyzzyneedlemarker'
    rows.append((f'Document {13370 if i == 1337 else i}', body))

# A few Italian rows with accents, so tokenisation and snippets are exercised on the
# language the bunker is actually for, not only on ASCII noise.
rows.append(('Città e caffè', 'La città è piena di caffè e perché no, anche di poesia.'))
rows.append(('Perché la fotosintesi', 'La fotosintesi clorofilliana è un processo biologico.'))

con.executemany("INSERT INTO docs(title, body) VALUES (?, ?)", rows)
con.commit()
con.execute("INSERT INTO docs(docs) VALUES('optimize')")
con.commit()
con.close()
print(f"built {out} ({out.stat().st_size // 1024} KB)")
