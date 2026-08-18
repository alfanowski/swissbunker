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

Record the outcome of each probe here as it completes, so the findings document can be
written from this file rather than from memory.

### P1 verdict

**Control condition only, 2026-08-19.** Chromium (Playwright) on macOS, Apple M4:
all 8 checks PASS over `http://` — including `dynamic_esm_import`, `fetch_sibling_file`,
`indexeddb_usable` and `opfs_usable`. This confirms the probe itself is sound; it says
nothing yet about `file://`, which is the condition that matters and must be run by hand.

Record: `results/p1-http-chromium-macos.json`

### P2 verdict — risk R1

_pending_

### P3 verdict — risk R6 and the latency budget

_pending_

### P4 verdict

_pending_

### P5 verdict — risk R7 and tier calibration

**Control condition only, 2026-08-19.** Chromium (Playwright) on macOS, Apple M4 / 16 GB:
all 7 checks PASS over `http://`.

Two findings worth carrying forward, both provisional until confirmed under `file://` and
on non-Apple hardware:

- **`maxBufferSize` = 4294967292 bytes (4.29 GB)**, not the ~2 GB that spec constraint V4
  assumes. Adapter reports `apple / metal-3` with `shader-f16` and `subgroups`. If this
  holds across machines, the tier table in spec §8.1 is too conservative and Tier 3 is
  reachable on Apple Silicon.
- **`webgpu_inside_blob_worker` PASS** — a Blob-URL worker acquired its own adapter, so
  inference can leave the main thread. The UI does not have to be designed around a
  blocking generate call.

`crossOriginIsolated: false` and no `SharedArrayBuffer`, as expected without COOP/COEP.

Record: `results/p5-http-chromium-macos.json`

### P6 verdict — risk R2

_pending_
