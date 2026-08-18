# Phase 0 — Feasibility spike

This directory answers one question: **can the Portable runtime exist?**

Everything here is throwaway. No production code may import from it.

## Order of execution

1. `tools/make-fixtures.sh` — generate the exFAT image and test data (~30 min, one time)
2. `tools/serve.sh` — start the http:// control baseline in a second terminal
3. Run probes P1 to P6 in order. Each one twice: from `file://` and from `http://localhost:8000`
4. Save every downloaded JSON into `results/`
5. Open `report.html` to build the matrix
6. Write the findings document into `../../docs/reports/`

## Probes

| Probe | Question | Blocks what if it fails |
|-------|----------|-------------------------|
| P1 | What forms of code loading survive a null origin? | Everything — run first |
| P2 | Can a `file://` page enumerate a directory and get `File` objects? | The whole Portable mode (R1) |
| P3 | Is `File.slice()` correct and fast on a multi-GB exFAT file? | The index format (R6) |
| P4 | Can SQLite query a 10 GB database through range reads? | The search engine |
| P5 | Are WebGPU and Web Workers available under a null origin? | Local inference |
| P6 | Can LLM weights load from `File` objects instead of HTTP? | The chat feature (R2) |

## P4 needs one extra step

The first five checks of P4 run as-is. The sixth — a real FTS5 query — needs the sql.js
bundle, which is not committed because it is derived:

```bash
cd spikes/phase-0
npm init -y && npm install sql.js@1.11.0
./tools/build-vendor.sh
```

## Naming of result files

`results/<probe>-<protocol>-<browser>-<os>.json`, e.g. `p3-file-chrome-macos.json`.
The harness names the first two segments; the operator appends browser and OS.

For P3 also append the medium: `-image` for the exFAT disk image, `-usb` for a real
external disk. Only the `-usb` numbers count towards the latency budget — the image
reproduces exFAT semantics, not USB latency.

## Rule

A probe that fails under **both** `file://` and `http://` has a bug in the probe.
Fix the probe. Only a `file://`-specific failure is a finding.

## Verdicts

All six probes ran on 2026-08-19 across chromium, chrome, firefox and webkit, in both
protocols — 48 records, all committed in `results/`. Full analysis in
[`docs/reports/2026-08-19-phase-0-findings.md`](../../docs/reports/2026-08-19-phase-0-findings.md).

**Decision: GO WITH CHANGES.**

### P1 — code loading

`file://` costs three capabilities: ES module import, `fetch` of a sibling file, and OPFS
(the last on Chromium and WebKit; Firefox keeps it). Classic script tags, inline
`WebAssembly.instantiate`, Blob-URL scripts, `localStorage` and `IndexedDB` all survive.
`isSecureContext` is true, which is why WebGPU and WebCrypto remain available.

**Consequence:** everything ships as classic IIFE with wasm inlined in base64.

### P2 — directory enumeration · risk R1

`webkitdirectory` returns valid `File` objects on **all four engines** under `file://`.
Seven files totalling 19.23 GB enumerated in **0 ms**. `showDirectoryPicker` is absent on
Firefox and WebKit, confirming that not depending on it was necessary rather than merely
prudent.

**R1 neutralised** for disk reading — the bet the whole architecture rests on.

### P3 — range reads · risk R6 and the latency budget

Byte-level correctness holds at every boundary, across the 4 GB line, and over 200 random
pages of a 12.88 GB file. **R6 neutralised.**

The decisive number: 1400 scattered 4 KB reads cost 297.8 ms against 7.6 ms for a single
contiguous 5.6 MB read — the same bytes at **39.2× the cost**. IVF over HNSW is confirmed.
Parallel reads beat sequential by only 1.23×, so the IVF reader stays sequential.

Caveat: measured on an exFAT disk image over internal NVMe. exFAT semantics are real, USB
latency is not.

### P4 — SQLite over range reads

Two findings, one bad and one better than expected.

- **`sql.js` has no FTS5** — `no such module: fts5` on all four engines. The full-text engine
  the spec is built on does not exist in the assumed library. New risk **R10**.
- **Synchronous blocking reads are possible.** A synchronous XHR against the Blob URL of a
  file slice returns the correct bytes in **0.7 ms**, on all four engines. Since SQLite's VFS
  demands synchronous reads and `file://` denies `SharedArrayBuffer`, this was the worst
  theoretical blocker — and it is not there.

Also: 39.7 MB read per simulated query is too high, dominated by the 1 MB chunk size, and the
lazy reader needs LRU eviction (risk **R13**).

### P5 — WebGPU and workers · risk R7

Chromium and Chrome: full pass under `file://`, including compute shader dispatch with a
verified numeric result, a 1 GB buffer allocation, and WebGPU **inside a Blob-URL worker** —
so inference can leave the main thread.

`maxBufferSize` measures **4.29 GB** on Apple M4, not the ~2 GB the spec assumed. **R7 is
reversed**: the constraint was too conservative.

Firefox and WebKit report no WebGPU, but these are Playwright builds that do not ship it.
That is a tooling limit, **not** evidence about Safari 26 or Firefox 147. Risk **R12**, to be
closed by hand.

### P6 — model weights · risk R2

Full pass on all four engines. `cache_api_accepts_file` works, so the cheap path for R2 is
open: a library's HTTP loader can be satisfied by pre-populating the Cache API. A 491 MB
`File` transfers to a worker with its size intact, and streams at ~4.4 GB/s.

**R2 drops from Medium/High to Low.**

## Contamination found and corrected

Playwright's Firefox build ships `pref("security.fileuri.strict_origin_policy", false)`,
which disables the exact policy this spike measures. The first Firefox pass reported 8/8 —
fiction. The runner now restores the preference; Firefox scores 6/8, in line with the other
engines. Those records were deleted, not amended.

Chromium and WebKit were checked for the equivalent: Playwright passes them no
`--allow-file-access-from-files` and no `--disable-web-security`. Their records are clean.

A second defect was mine: `webgpu_inside_blob_worker` scored PASS on engines with no WebGPU,
because the check returned the failure *reason* as a string and `Probe.check` treats any
non-false value as success. Fixed to return `false`.
