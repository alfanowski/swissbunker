# Changelog

All notable changes to this project are documented here.
Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Design specification (`docs/specs/2026-08-18-swissbunker-design.md`)
- Phase 0 implementation plan (`docs/plans/2026-08-19-phase-0-feasibility-spike.md`)
- Phase 0 spike: probe harness, six probes, automated cross-engine runner, matrix builder
- Phase 0 findings and go/no-go decision (`docs/reports/2026-08-19-phase-0-findings.md`)
- Phase 1 implementation plan (`docs/plans/2026-08-19-phase-1-reader.md`)
- Phase 1 reader: ByteSource, LRU page cache, read-only SQLite VFS, FTS5 search, IIFE bundle,
  minimal UI — 45 unit tests, 7 conformance checks under `file://`
- Phase 1 verification report (`docs/reports/2026-08-19-phase-1-verification.md`)
- Repository scaffolding, proprietary license, CI skeleton

### Fixed
- Reader init died under `file://` because Emscripten resolves the wasm filename before
  reading `wasmBinary`; `locateFile` now supplied
- Ranking a frequent term cost 2120 ms; a measured cutoff drops it to 1.8 ms

### Changed
- Spec constraint V4: `maxBufferSize` is measured at runtime, not assumed to be ~2 GB
- Spec §6.4: `sql.js` replaced — it ships without FTS5
- Spec §10: encrypted personal store moves from OPFS to IndexedDB
- Spec §14: risk register carries measured probabilities and four new risks (R10-R13)

[Unreleased]: https://github.com/alfanowski/swissbunker/commits/main
