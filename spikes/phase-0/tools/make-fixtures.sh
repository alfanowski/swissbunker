#!/usr/bin/env bash
# Generates the test fixtures for Phase 0.
#
# The exFAT disk image is the key trick: it reproduces exFAT's *semantics* (no permissions,
# no symlinks, its 32-bit heritage, allocation behaviour) without needing physical hardware.
# It does NOT reproduce USB *latency* — the real disk is still required for P3's timing
# numbers. Semantics first, timing later.
set -euo pipefail

FIXTURES="${1:-$HOME/swissbunker-fixtures}"
mkdir -p "$FIXTURES"

echo "==> Creating 120 GB sparse exFAT image (uses only the space actually written)"
if [ ! -f "$FIXTURES/exfat-test.sparseimage" ]; then
  hdiutil create -size 120g -fs exFAT -volname SWISSTEST -type SPARSE \
    -layout GPTSPUD "$FIXTURES/exfat-test" >/dev/null
fi

echo "==> Mounting"
hdiutil attach "$FIXTURES/exfat-test.sparseimage" >/dev/null
MOUNT="/Volumes/SWISSTEST"

echo "==> Generating a 12 GB file with verifiable content"
# Every 4096-byte page begins with its own page number in ASCII, so a range read can be
# checked for correctness and not merely for "it returned some bytes".
python3 - "$MOUNT/large-verifiable.bin" <<'PY'
import sys
path = sys.argv[1]
PAGE = 4096
PAGES = 3 * 1024 * 1024          # 12 GB
with open(path, 'wb') as f:
    for i in range(PAGES):
        header = f"PAGE:{i:012d}:".encode('ascii')
        f.write(header + bytes(PAGE - len(header)))
        if i % 262144 == 0:
            print(f"  {i * PAGE / 1e9:.1f} GB", flush=True)
PY

echo "==> Generating a 10 GB SQLite database with an FTS5 index"
python3 - "$MOUNT/fts-test.sqlite" <<'PY'
import sqlite3, sys, random, string
path = sys.argv[1]
con = sqlite3.connect(path)
con.execute("PRAGMA journal_mode=OFF")
con.execute("PRAGMA synchronous=OFF")
con.execute("PRAGMA page_size=4096")
con.execute("CREATE VIRTUAL TABLE docs USING fts5(title, body)")
words = [''.join(random.choices(string.ascii_lowercase, k=random.randint(3, 11)))
         for _ in range(20000)]
# A known needle planted at a known row lets the probe assert on an exact result
# instead of on "some rows came back".
BATCH, TOTAL = 5000, 2_000_000
rows = []
for i in range(TOTAL):
    body = ' '.join(random.choices(words, k=180))
    if i == 1_337_000:
        body += ' xyzzyneedlemarker'
    rows.append((f'Document {i}', body))
    if len(rows) >= BATCH:
        con.executemany("INSERT INTO docs(title, body) VALUES (?, ?)", rows)
        rows.clear()
        if i % 250000 == 0:
            con.commit(); print(f"  {i:,} rows", flush=True)
if rows:
    con.executemany("INSERT INTO docs(title, body) VALUES (?, ?)", rows)
con.commit()
con.execute("INSERT INTO docs(docs) VALUES('optimize')")
con.commit()
con.close()
PY

echo "==> Generating a 121 MB companion database that fits in memory"
# The big database and this one carry the same schema and the same planted needle. Having
# both separates two questions that would otherwise be conflated: "does FTS5 work inside
# WASM?" and "can a multi-gigabyte file be read lazily?" A failure then points at one of
# them instead of both.
python3 - "$MOUNT/fts-small.sqlite" <<'PY2'
import sqlite3, sys, random, string
con = sqlite3.connect(sys.argv[1])
con.execute("PRAGMA journal_mode=OFF"); con.execute("PRAGMA synchronous=OFF")
con.execute("PRAGMA page_size=4096")
con.execute("CREATE VIRTUAL TABLE docs USING fts5(title, body)")
words = [''.join(random.choices(string.ascii_lowercase, k=random.randint(3, 11)))
         for _ in range(5000)]
rows = []
for i in range(40000):
    body = ' '.join(random.choices(words, k=180))
    if i == 13370:
        body += ' xyzzyneedlemarker'
    rows.append((f'Document {i}', body))
con.executemany("INSERT INTO docs(title, body) VALUES (?, ?)", rows)
con.commit()
con.execute("INSERT INTO docs(docs) VALUES('optimize')")
con.commit()
con.close()
PY2

echo "==> Fetching a small real ZIM for format realism"
# Kiwix filenames carry a date suffix that changes monthly, so this URL rots. It is
# optional: no probe depends on the ZIM, it is only there for format realism.
curl -fL --retry 3 -o "$MOUNT/wikimed.zim" \
  "https://download.kiwix.org/zim/wikipedia/wikipedia_en_medicine_nopic.zim" \
  || echo "    WARNING: ZIM download failed — the other probes can still run"

echo
echo "Fixtures ready at $MOUNT"
ls -lh "$MOUNT"
echo
echo "Unmount with: hdiutil detach $MOUNT"
