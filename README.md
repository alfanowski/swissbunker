<div align="center">

# SwissBunker

**A portable, offline knowledge bunker on a single disk.**
Plug it into any computer. No installation. No admin rights. No internet.

`Status: phase 1 reader working` · `License: proprietary` · `Runtime: browser + WebGPU`

</div>

---

## What it is

SwissBunker turns an external disk into a self-contained, indexed, queryable copy of
humanity's public-domain knowledge — Wikipedia, books, practical manuals, Q&A archives and
offline maps — searchable through a hybrid retrieval engine and answerable by a local LLM
that runs **in the host computer's browser**, using its GPU.

You plug the disk into a random PC, open one page, and you have everything. Nothing is
installed on that machine. Nothing is left behind on it.

## The problem it solves

Existing offline-knowledge projects assume a home server: Docker, Proxmox, a fixed machine,
a network you control, and an administrator's patience. That is a fine solution to a
different problem.

SwissBunker assumes **none of that**. The bunker is bound to the disk, not to a machine.

## How it works

One codebase, two runtime modes, selected automatically by capability detection.

```
                    ┌──────────────── THE DISK ────────────────┐
                    │  bin/     native daemons (per OS)         │
   curl | sh  ────►  │  app/     dashboard (JS + WASM)          │
   (bootstrap)      │  content/ monolithic corpora              │
                    │  index/   full-text + vector indexes      │
                    │  models/  embedder, reranker, LLM tiers    │
                    └──────────────────────────────────────────┘
                              │                      │
                    ┌─────────▼────────┐   ┌─────────▼─────────┐
                    │    CONNECTED     │   │     PORTABLE      │
                    │ daemon on :7777  │   │  START.html       │
                    │ acquire · index  │   │  read · search    │
                    │ embed · update   │   │  RAG · WebGPU LLM │
                    └─────────┬────────┘   └─────────┬─────────┘
                              └──────► BROWSER ◄─────┘
```

**Connected** — a Rust daemon that lives *on the disk* (never installed on the host) serves
the dashboard over `localhost`. This is where you download corpora and build indexes.

**Portable** — no native process at all. You open `START.html` from the disk and everything
runs client-side: search, retrieval, and LLM inference over WebGPU. This is the mode that
must work on a computer you have never seen before.

## Design highlights

| Decision | Rationale |
|---|---|
| No File System Access API | The bunker is read-only, so `webkitdirectory` + `File.slice()` gives universal random access across every browser, not just Chromium |
| Few monolithic files | exFAT is the only cross-platform writable filesystem, and it is hostile to many small files |
| BM25 over 100% of the corpus, dense vectors over a selective subset | Embedding 220M chunks is weeks of GPU time; full-text indexing is hours of I/O. Asymmetric coverage is what makes the build feasible on a laptop |
| IVF instead of HNSW | HNSW's dependent random hops are latency-bound over USB. IVF does a handful of large contiguous reads |
| SQLite FTS5 instead of Tantivy | It is the only serious text index readable from the browser via WASM. The runtime constraint dictates the index format |
| One read path, both modes | The daemon builds indexes but never serves a search. Client-side code is the sole reader, always — eliminating the class of bugs where search works at home and breaks in the field |
| The model never answers unsourced | Every claim maps to a retrieved passage with a clickable anchor. Where you cannot verify anything online, a hallucination is worse than "I don't know" |

## Documentation

| Document | Contents |
|---|---|
| [Design specification](docs/specs/2026-08-18-swissbunker-design.md) | Full architecture, data and index formats, retrieval pipeline, UX, budgets, risk register |
| [Changelog](CHANGELOG.md) | Release history |

The internal specification is written in Italian; source code, identifiers, comments and
public documentation are in English.

## Content

Only public-domain and openly-licensed material. Sources include Wikipedia, Project
Gutenberg, Wikisource, Wikibooks, Stack Exchange, iFixit, WikiMed, Khan Academy,
OpenStreetMap and arXiv metadata. Each retains its own license.

Pirated or copyright-infringing corpora are explicitly out of scope and will not be
supported.

## Status

**Phase 0 complete: GO with changes.** The feasibility spike ran six probes across four
engines in both `file://` and `http://` conditions — 48 records, all committed.

The Portable runtime is viable: `webkitdirectory` returns usable `File` objects on every
engine under `file://`, a 12.88 GB file reads correctly at any offset, and synchronous
blocking reads — the worst theoretical blocker — complete in 0.7 ms. IVF over HNSW is
confirmed by a **39× margin**.

Three assumptions were wrong and the spec has been corrected: `sql.js` ships without FTS5,
OPFS is denied under `file://` on Chromium and WebKit, and WebGPU's `maxBufferSize` is 4.29 GB
rather than the ~2 GB assumed.

See the [Phase 0 findings](docs/reports/2026-08-19-phase-0-findings.md), including a full
account of what the spike did **not** prove.

**Phase 1: the reader works.** From a page opened with `file://`, with nothing installed on
the machine, a 6.22 GB SQLite FTS5 index opens in **19 ms** and answers a search in **17 ms**,
reading **0.014% of the file**. 45 unit tests, 7 conformance checks.

Two problems nearly sank it, neither in the plan: initialisation died only under a null
origin, because Emscripten resolves the wasm filename before it reads the bytes you handed
it; and ranking a term present in 35,575 documents cost 2120 ms, which took four experiments
to isolate and disproved three hypotheses on the way.

See the [Phase 1 verification](docs/reports/2026-08-19-phase-1-verification.md) — including
what it does **not** prove, starting with the fact that the corpus is synthetic.

## License

Proprietary. All rights reserved. See [LICENSE](LICENSE).

This repository is public so the work can be read and reviewed. It is **not** open source:
no permission is granted to use, copy, modify or distribute it. Forking through GitHub's
interface, as the GitHub Terms of Service allow on any public repository, does not grant
any of those rights.
