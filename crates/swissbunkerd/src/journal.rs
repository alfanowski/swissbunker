//! Durable record of how far each job got.
//!
//! Download, extraction and indexing all need to resume, and each resumes differently: a
//! download from a byte offset, an extraction from a document index, an index from a segment.
//! One module that knows how to record "where I was" for all three avoids three divergent
//! implementations of the same problem — and divergence is always discovered after a crash,
//! which is the worst possible moment.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

/// The stages every corpus passes through.
///
/// An enum rather than a free string, so a typo is a compile error instead of a job that
/// silently never resumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Download,
    Extract,
    Index,
    Verify,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Download => "download",
            Stage::Extract => "extract",
            Stage::Index => "index",
            Stage::Verify => "verify",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "download" => Some(Stage::Download),
            "extract" => Some(Stage::Extract),
            "index" => Some(Stage::Index),
            "verify" => Some(Stage::Verify),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobId(pub String);

/// One row of the pipeline, as the dashboard needs to render it.
#[derive(Debug, Clone)]
pub struct JobProgress {
    pub job: JobId,
    pub stage: Stage,
    pub position: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
}

pub struct Journal {
    // A Mutex rather than a connection pool: writes happen every few seconds at most, and a
    // pool would add a dependency and a failure mode to solve contention that does not exist.
    conn: Mutex<Connection>,
}

impl Journal {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening journal at {}", path.display()))?;

        // SQLite rather than a JSON file that gets rewritten: a rewrite interrupted halfway
        // leaves a truncated file, and the one moment this data matters is exactly the moment
        // something was interrupted halfway.
        //
        // EXCLUSIVE locking is REQUIRED on exFAT, not a tuning choice. Measured: with default
        // settings every write to a database on exFAT fails with SQLITE_READONLY_DBMOVED
        // (1032, "database file has moved"). SQLite guards against a swapped file by checking
        // the inode, and exFAT has no stable inodes — so the check misfires on every write.
        //
        // EXCLUSIVE holds the lock for the connection's lifetime and stops SQLite re-checking,
        // while KEEPING the rollback journal and therefore atomicity. The other two settings
        // that also work — journal_mode=MEMORY and journal_mode=OFF — buy the same result by
        // discarding crash safety, which in a crash-recovery journal would be self-defeating.
        //
        // The cost is that only one process may hold the journal open. The daemon is the sole
        // writer by design, so this constraint is free here; anything else that needs to read
        // progress must go through the daemon's API.
        conn.pragma_update(None, "locking_mode", "EXCLUSIVE")
            .context("setting exclusive locking, required for exFAT")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS progress (
                 job       TEXT    NOT NULL,
                 stage     TEXT    NOT NULL,
                 position  INTEGER NOT NULL DEFAULT 0,
                 total     INTEGER NOT NULL DEFAULT 0,
                 done      INTEGER NOT NULL DEFAULT 0,
                 error     TEXT,
                 PRIMARY KEY (job, stage)
             );",
        )
        .context("creating the progress table")?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Record progress. Idempotent, and clears any previous error: a job that is visibly
    /// moving is not failing, and the UI must not keep showing an error for it.
    pub fn mark(&self, job: &JobId, stage: Stage, position: u64, total: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO progress (job, stage, position, total, done, error)
                  VALUES (?1, ?2, ?3, ?4, 0, NULL)
             ON CONFLICT(job, stage) DO UPDATE
                  SET position = excluded.position,
                      total    = excluded.total,
                      error    = NULL",
            params![job.0, stage.as_str(), position as i64, total as i64],
        )?;
        Ok(())
    }

    /// Where to resume from, or None if this stage never started.
    pub fn resume_point(&self, job: &JobId, stage: Stage) -> Result<Option<u64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT position FROM progress WHERE job = ?1 AND stage = ?2")?;
        let mut rows = stmt.query(params![job.0, stage.as_str()])?;
        Ok(match rows.next()? {
            Some(r) => Some(r.get::<_, i64>(0)? as u64),
            None => None,
        })
    }

    pub fn progress(&self, job: &JobId, stage: Stage) -> Result<Option<JobProgress>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT position, total, done, error FROM progress WHERE job = ?1 AND stage = ?2",
        )?;
        let mut rows = stmt.query(params![job.0, stage.as_str()])?;
        Ok(match rows.next()? {
            Some(r) => Some(JobProgress {
                job: job.clone(),
                stage,
                position: r.get::<_, i64>(0)? as u64,
                total: r.get::<_, i64>(1)? as u64,
                done: r.get::<_, i64>(2)? != 0,
                error: r.get::<_, Option<String>>(3)?,
            }),
            None => None,
        })
    }

    /// Every recorded stage of every job, for rendering the whole pipeline in one call.
    pub fn all_progress(&self) -> Result<Vec<JobProgress>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT job, stage, position, total, done, error FROM progress ORDER BY job, stage",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (job, stage, position, total, done, error) = row?;
            // A row whose stage name is unrecognised comes from a newer version of the
            // daemon. Skipping it is better than failing: an old daemon should still be able
            // to report on the stages it does understand.
            if let Some(stage) = Stage::from_str(&stage) {
                out.push(JobProgress {
                    job: JobId(job),
                    stage,
                    position: position as u64,
                    total: total as u64,
                    done: done != 0,
                    error,
                });
            }
        }
        Ok(out)
    }

    pub fn complete(&self, job: &JobId, stage: Stage) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO progress (job, stage, position, total, done, error)
                  VALUES (?1, ?2, 0, 0, 1, NULL)
             ON CONFLICT(job, stage) DO UPDATE SET done = 1, error = NULL",
            params![job.0, stage.as_str()],
        )?;
        Ok(())
    }

    pub fn is_complete(&self, job: &JobId, stage: Stage) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT done FROM progress WHERE job = ?1 AND stage = ?2")?;
        let mut rows = stmt.query(params![job.0, stage.as_str()])?;
        Ok(match rows.next()? {
            Some(r) => r.get::<_, i64>(0)? != 0,
            None => false,
        })
    }

    /// Record a failure WITHOUT discarding the resume point.
    ///
    /// Throwing away the position on error would turn every transient network fault into a
    /// restart from zero, which on a multi-hour download is the difference between a product
    /// and a toy.
    pub fn failed(&self, job: &JobId, stage: Stage, error: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO progress (job, stage, position, total, done, error)
                  VALUES (?1, ?2, 0, 0, 0, ?3)
             ON CONFLICT(job, stage) DO UPDATE SET error = excluded.error, done = 0",
            params![job.0, stage.as_str(), error],
        )?;
        Ok(())
    }

    pub fn last_error(&self, job: &JobId, stage: Stage) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT error FROM progress WHERE job = ?1 AND stage = ?2")?;
        let mut rows = stmt.query(params![job.0, stage.as_str()])?;
        Ok(match rows.next()? {
            Some(r) => r.get::<_, Option<String>>(0)?,
            None => None,
        })
    }

    /// Forget a stage entirely, so it starts over.
    ///
    /// Needed when a hash check fails: the bytes already on disk are wrong, so resuming from
    /// the recorded offset would append good data onto bad and keep failing the hash for ever.
    pub fn reset(&self, job: &JobId, stage: Stage) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM progress WHERE job = ?1 AND stage = ?2",
            params![job.0, stage.as_str()],
        )?;
        Ok(())
    }
}
