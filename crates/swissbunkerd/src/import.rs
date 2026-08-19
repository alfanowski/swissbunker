//! Turning content that is already on the disk into indexable documents.
//!
//! The Forge does not fetch anything: the operator puts corpora on the disk and this module
//! reads them. That inverts the usual shape — there is no download to resume, only a file to
//! stream — and it means the import path has to be forgiving about what it is handed.
//!
//! JSONL is the native format, deliberately: one JSON object per line streams without loading
//! the corpus into memory, survives a truncated last line, and anything can be converted into
//! it with a few lines of script. A format the operator can produce from a shell is worth more
//! here than one that is faster to parse.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::index::Document;
use crate::journal::{JobId, Journal, Stage};
use crate::manifest::ImportanceSignal;

/// One line of a JSONL corpus.
///
/// Only `title` and `body` are required. Everything else has a sensible default, so a corpus
/// can be produced by a five-line script rather than by reading a specification.
#[derive(Debug, Deserialize)]
struct JsonlDocument {
    title: String,
    body: String,
    #[serde(default)]
    source_path: Option<String>,
    /// When absent, source order decides — see `ImportanceSignal::SourceOrder`.
    #[serde(default)]
    importance: Option<f64>,
}

/// What an import produced, and how much of it was usable.
#[derive(Debug, Clone)]
pub struct ImportStats {
    pub documents: u64,
    /// Lines that could not be parsed. Counted rather than fatal: one malformed line in a
    /// million-line corpus should not throw away the other 999,999.
    pub skipped: u64,
    pub signal: ImportanceSignal,
}

/// How often progress reaches the journal.
const JOURNAL_EVERY: u64 = 2_000;

/// Read a JSONL corpus into documents, calling `on_document` for each.
///
/// Streaming rather than collecting: a corpus is expected to be larger than memory, and the
/// index builder consumes an iterator anyway.
///
/// Importance defaults to descending source order, which is not arbitrary: Kiwix's ZIM
/// editions select articles by popularity, so the order they appear in is already a signal
/// (spec §6.5). An explicit `importance` field overrides it.
pub fn import_jsonl(
    path: &Path,
    journal: &Journal,
    job: &JobId,
    mut on_document: impl FnMut(Document) -> Result<()>,
) -> Result<ImportStats> {
    let file = File::open(path).with_context(|| format!("opening corpus at {}", path.display()))?;
    let reader = BufReader::new(file);

    let mut documents: u64 = 0;
    let mut skipped: u64 = 0;
    let mut any_explicit_importance = false;

    for (line_no, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("reading line {}", line_no + 1))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed: JsonlDocument = match serde_json::from_str(trimmed) {
            Ok(d) => d,
            Err(_) => {
                // A malformed line is counted and skipped. The alternative — failing the whole
                // import — would mean one bad character costs hours of work, and the operator
                // usually cannot fix a corpus they did not generate.
                skipped += 1;
                continue;
            }
        };

        if parsed.title.trim().is_empty() && parsed.body.trim().is_empty() {
            skipped += 1;
            continue;
        }

        if parsed.importance.is_some() {
            any_explicit_importance = true;
        }

        // Descending by position: the first document in the file gets the highest importance,
        // so source order survives into insertion order.
        let importance = parsed.importance.unwrap_or_else(|| -(documents as f64));

        let source_path = parsed
            .source_path
            .unwrap_or_else(|| format!("{}#{}", path.display(), line_no + 1));

        on_document(Document {
            title: parsed.title,
            body: parsed.body,
            source_path,
            importance,
        })?;

        documents += 1;
        if documents.is_multiple_of(JOURNAL_EVERY) {
            journal.mark(job, Stage::Extract, documents, documents)?;
        }
    }

    journal.complete(job, Stage::Extract)?;

    Ok(ImportStats {
        documents,
        skipped,
        // The manifest must say which signal was used, because it decides whether unranked
        // results mean "the most important" or merely "some".
        signal: if any_explicit_importance {
            // Explicit, not Pageviews: the operator gave numbers, and we have no idea what
            // they measure. Naming a provenance we cannot verify would put a lie in the
            // manifest that the UI would then repeat to the user.
            ImportanceSignal::Explicit
        } else {
            ImportanceSignal::SourceOrder
        },
    })
}

/// Detect what a file is, so the operator does not have to declare it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusFormat {
    Jsonl,
    Zim,
    Unknown,
}

/// Identify a corpus by its magic bytes, falling back to the extension.
///
/// Content first, name second: a file renamed by hand is common on a disk the operator fills
/// themselves, and trusting the extension alone would fail confusingly later.
pub fn detect_format(path: &Path) -> Result<CorpusFormat> {
    use std::io::Read;
    let mut head = [0u8; 8];
    let read = File::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .read(&mut head)
        .unwrap_or(0);

    // ZIM files start with the magic number 0x44D495A (little-endian), per the format spec.
    if read >= 4 && u32::from_le_bytes([head[0], head[1], head[2], head[3]]) == 0x44D_495A {
        return Ok(CorpusFormat::Zim);
    }
    // JSONL has no magic; a first non-space byte of '{' is the strongest signal available.
    if read >= 1 {
        let first = head[..read].iter().find(|b| !b.is_ascii_whitespace());
        if first == Some(&b'{') {
            return Ok(CorpusFormat::Jsonl);
        }
    }

    Ok(match path.extension().and_then(|e| e.to_str()) {
        Some("jsonl") | Some("ndjson") => CorpusFormat::Jsonl,
        Some("zim") => CorpusFormat::Zim,
        _ => CorpusFormat::Unknown,
    })
}

/// Import any recognised corpus.
///
/// ZIM support is not implemented yet and says so, rather than pretending: the format needs a
/// libzim binding, and this project's rule is to inspect an API before writing code against
/// it. Until then the operator converts to JSONL, which a short script can do.
pub fn import(
    path: &Path,
    journal: &Journal,
    job: &JobId,
    on_document: impl FnMut(Document) -> Result<()>,
) -> Result<ImportStats> {
    match detect_format(path)? {
        CorpusFormat::Jsonl => import_jsonl(path, journal, job, on_document),
        CorpusFormat::Zim => bail!(
            "{} is a ZIM archive, which this build cannot read yet. \
             Convert it to JSONL (one {{\"title\":…,\"body\":…}} per line) and import that.",
            path.display()
        ),
        CorpusFormat::Unknown => bail!(
            "cannot tell what {} is. Supported: JSONL (.jsonl/.ndjson) and ZIM (.zim).",
            path.display()
        ),
    }
}
