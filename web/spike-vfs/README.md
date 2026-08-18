# Phase 1 · Task 1 — SQLite engine decision

Throwaway spike. Nothing here ships.

## Question

Phase 0 chose `@sqlite.org/sqlite-wasm` because it exposes `sqlite3_vfs_register`, but never
evaluated `wa-sqlite` — a library designed specifically so that a VFS is written by
subclassing a JavaScript base class instead of filling C structs. Choosing on half the
evidence was not good enough to build three weeks on.

## Method

Identical binary criterion for both: open a real 1.5 MB FTS5 database and answer a query
whose correct result is known in advance — a token planted in exactly one row out of two
thousand, titled `Document 13370` — with **every read routed through a custom VFS** and
counted.

Reads are synchronous via `fs.readSync`. In the browser the same call becomes
`FileSource.readSync` (synchronous XHR over a Blob URL, 0.7 ms measured in Phase 0). The VFS
logic is unchanged either way, which is exactly what `ByteSource` exists to guarantee.

## Result

| | `@sqlite.org/sqlite-wasm` 3.53.0 | `wa-sqlite` 1.0.0 |
|---|---|---|
| VFS ergonomics | fill `sqlite3_vfs` + `sqlite3_io_methods`, then `vfs.installVfs` | subclass `VFS.Base`, override methods |
| Lines for a minimal read-only VFS | 105 | 71 |
| Database opened through the custom VFS | **yes** | **yes** |
| **FTS5 present** | **yes** | **no — `no such module: fts5`** |
| `fts5` string in any shipped wasm | 1 | **0 in both builds** |
| Needle query answered correctly | **yes** — `Document 13370` | never reached |
| Reads to answer the query | **9** | — |
| Bytes read | **32 KB of 1.5 MB (2.1%)** | — |
| wasm size | 844 KB | 545 KB |
| Module format | ESM — needs IIFE bundling | ESM — needs IIFE bundling |

## Decision

**`@sqlite.org/sqlite-wasm`.**

`wa-sqlite` is the nicer VFS API and the smaller binary, and its VFS genuinely worked — the
database opened, SQLite read the header and the schema through the custom methods, and the
run died at `prepare_v2` on `no such module: fts5`. Neither of its shipped wasm builds
contains FTS5 at all. Without full-text search the library cannot do the one job this
project needs, and rebuilding it from source with `-DSQLITE_ENABLE_FTS5` would mean owning
a custom toolchain forever.

The official package costs about 30 more lines of C-struct plumbing, written once.

## What this also proves

The result is stronger than a library choice. **A query answered correctly after reading
2.1% of the database** is the architecture working end to end: SQLite drove a custom VFS,
the VFS served synchronous ranged reads, and the file was never loaded. On a larger index
the fraction falls further, because a B-tree's depth grows logarithmically while the file
grows linearly.

## Also learned

Both implementations were written twice. The first `wa-sqlite` attempt called
`this.dataView()` and passed `name` to a constructor — neither exists. `VFS.Base` has no
constructor at all, and its out-parameters arrive as `DataView` already.

This is the second time in this project that code written against an unread API had to be
thrown away, after `FS.createLazyFile` in Phase 0. The rule in the Phase 1 plan holds:
**print the API first, write code second.**

Both libraries also `fetch` their own `.wasm` at init, which fails in Node on a file path
and would fail identically under a null origin. Both accept `wasmBinary` instead — the same
workaround Phase 0 already needed.
