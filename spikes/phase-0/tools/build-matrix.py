#!/usr/bin/env python3
"""Render the records in results/ as the markdown matrix the findings document needs.

report.html does the same job interactively; this exists so the matrix can be pasted into
a document and reviewed in a diff, which a browser table cannot be.

Usage:
    python3 tools/build-matrix.py            # every probe
    python3 tools/build-matrix.py p3 p5      # only these
"""
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RESULTS = ROOT / "results"
ENGINE_ORDER = ["chromium", "chrome", "firefox", "webkit"]


def load():
    """Return {probe: {(engine, protocol): record}}, skipping unparseable files."""
    out = {}
    for f in sorted(RESULTS.glob("p*.json")):
        parts = f.stem.split("-")
        if len(parts) < 4:
            continue
        probe, protocol, engine = parts[0], parts[1], parts[2]
        try:
            rec = json.loads(f.read_text())
        except json.JSONDecodeError:
            print(f"<!-- skipped unparseable {f.name} -->", file=sys.stderr)
            continue
        out.setdefault(probe, {})[(engine, protocol)] = rec
    return out


def cell(check):
    if check is None:
        return "—"
    if check["ok"]:
        return "**PASS**"
    detail = check["detail"]
    text = detail if isinstance(detail, str) else json.dumps(detail)
    # Keep the reason: a bare "fail" forces the reader back to the raw JSON.
    return "fail<br><sub>" + text[:44].replace("|", "\\|") + "</sub>"


def render(probe, records):
    keys = [(e, p) for e in ENGINE_ORDER for p in ("file", "http") if (e, p) in records]
    if not keys:
        return ""
    title = next(iter(records.values()))["title"]
    lines = [f"### {probe} — {title}", ""]

    header = "| check | " + " | ".join(f"{e}<br><sub>{p}</sub>" for e, p in keys) + " |"
    lines += [header, "|" + "---|" * (len(keys) + 1)]

    names = []
    for rec in records.values():
        for n in rec["checks"]:
            if n not in names:
                names.append(n)
    for n in names:
        lines.append(f"| `{n}` | " + " | ".join(cell(records[k]["checks"].get(n)) for k in keys) + " |")

    # Measurements only make sense per engine, and only where they exist.
    mnames = []
    for rec in records.values():
        for m in rec["measurements"]:
            if m not in mnames:
                mnames.append(m)
    if mnames:
        lines += ["", "| measurement (p50 / p95 ms) | " +
                  " | ".join(f"{e}<br><sub>{p}</sub>" for e, p in keys) + " |",
                  "|" + "---|" * (len(keys) + 1)]
        for m in mnames:
            row = []
            for k in keys:
                v = records[k]["measurements"].get(m)
                row.append(f"{v['p50']} / {v['p95']}" if v else "—")
            lines.append(f"| `{m}` | " + " | ".join(row) + " |")

    lines.append("")
    return "\n".join(lines)


data = load()
wanted = sys.argv[1:] or sorted(data)
for probe in wanted:
    if probe in data:
        print(render(probe, data[probe]))
