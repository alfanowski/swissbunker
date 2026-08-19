//! Builds the FTS5 index the browser-side Reader queries.
//!
//! The one requirement that shapes this module is spec §6.5: documents go in
//! **most-important-first**. FTS5 returns unranked matches in rowid order, and phase 1
//! measured that roughly half of real searches take the unranked path — so insertion order
//! IS result order for half of everything a user does. Getting it wrong does not break any
//! test; it just quietly makes the product worse.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use std::time::Instant;

use crate::journal::{JobId, Journal, Stage};

/// One indexable document.
#[derive(Debug, Clone)]
pub struct Document {
    pub title: String,
    pub body: String,
    /// Where this came from inside the source archive, so a reader can open the original.
    pub source_path: String,
    /// Higher is more important. Decides insertion order, and therefore result order for
    /// every search that is too broad to rank.
    pub importance: f64,
}

#[derive(Debug, Clone)]
pub struct IndexStats {
    pub documents: u64,
    pub bytes: u64,
    pub build_secs: f64,
}

/// How often progress reaches the journal. Every document would make the journal the
/// bottleneck; too rarely and a crash loses real work.
const JOURNAL_EVERY: u64 = 2_000;
/// Build an index at `out` from `docs`.
///
/// Two passes, because importance is not known until every document has been seen and the
/// corpus does not fit in memory:
///
///   1. Stage every document into an ordinary table, recording its importance.
///   2. Read that table back `ORDER BY importance DESC` and insert into FTS5.
///
/// The cost is temporary disk equal to the corpus text, and it buys the property that makes
/// unranked search useful. A single-pass build would be faster and would produce an index
/// whose broad searches return arbitrary documents.
///
/// KNOWN LIMIT: each pass runs in a single transaction, so an interrupted build restarts
/// from the beginning of its pass rather than from the journal position. That is acceptable
/// while corpora are measured in gigabytes and passes in minutes; it needs revisiting before
/// a corpus takes hours, and the journal already records enough to support it.
pub fn build_index(
    docs: impl Iterator<Item = Document>,
    out: &Path,
    journal: &Journal,
    job: &JobId,
) -> Result<IndexStats> {
    let started = Instant::now();

    // A rebuild replaces: appending to an existing index would leave every document twice,
    // and searches would still work, just wrongly. Silent duplication is worse than an error.
    if out.exists() {
        std::fs::remove_file(out).with_context(|| format!("removing {}", out.display()))?;
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let mut conn =
        Connection::open(out).with_context(|| format!("creating index at {}", out.display()))?;

    // Required on exFAT: without it every write fails with SQLITE_READONLY_DBMOVED, because
    // SQLite compares inodes to detect a swapped file and exFAT has none that are stable.
    // See constraint V8 in the spec.
    conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
    // The index is rebuilt from scratch on failure, so durability during the build buys
    // nothing: there is no partial state worth recovering, only a file to delete.
    conn.pragma_update(None, "synchronous", "OFF")?;
    conn.pragma_update(None, "page_size", 4096)?;

    conn.execute_batch(
        "CREATE TABLE staging (
             title       TEXT NOT NULL,
             body        TEXT NOT NULL,
             source_path TEXT NOT NULL,
             importance  REAL NOT NULL
         );
         -- Schema pinned by tests on both sides: the Reader queries `docs` and calls
         -- snippet(docs, 1, …), where 1 is body. Changing the column order breaks the
         -- browser silently.
         CREATE VIRTUAL TABLE docs USING fts5(title, body, source_path UNINDEXED);",
    )
    .context("creating index schema")?;

    // --- pass 1: stage -------------------------------------------------------------------
    let mut staged: u64 = 0;
    {
        let tx = conn.transaction()?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO staging (title, body, source_path, importance) VALUES (?1,?2,?3,?4)",
            )?;
            for d in docs {
                insert.execute(params![d.title, d.body, d.source_path, d.importance])?;
                staged += 1;
                if staged.is_multiple_of(JOURNAL_EVERY) {
                    // total is unknown while streaming, so it is reported as the count so far:
                    // an honest "N done" beats a fabricated percentage.
                    journal.mark(job, Stage::Index, staged, staged)?;
                }
            }
        }
        tx.commit()?;
    }

    // --- pass 2: insert in importance order ----------------------------------------------
    // The ORDER BY is the entire point of the two-pass design.
    let mut inserted: u64 = 0;
    {
        let tx = conn.transaction()?;
        {
            let mut read = tx.prepare(
                "SELECT title, body, source_path FROM staging ORDER BY importance DESC, rowid ASC",
            )?;
            let mut insert =
                tx.prepare("INSERT INTO docs (title, body, source_path) VALUES (?1,?2,?3)")?;

            let mut rows = read.query([])?;
            while let Some(row) = rows.next()? {
                let title: String = row.get(0)?;
                let body: String = row.get(1)?;
                let source_path: String = row.get(2)?;
                insert.execute(params![title, body, source_path])?;
                inserted += 1;
                if inserted.is_multiple_of(JOURNAL_EVERY) {
                    journal.mark(job, Stage::Index, inserted, staged)?;
                }
            }
        }
        tx.commit()?;
    }

    // Staging has served its purpose; VACUUM reclaims the space so the disk budget in the
    // wizard is not off by the size of the corpus text.
    conn.execute_batch("DROP TABLE staging;")?;
    conn.execute("INSERT INTO docs(docs) VALUES('optimize')", [])?;
    conn.execute_batch("VACUUM;")?;
    drop(conn);

    journal.complete(job, Stage::Index)?;

    let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    Ok(IndexStats {
        documents: inserted,
        bytes,
        build_secs: started.elapsed().as_secs_f64(),
    })
}
