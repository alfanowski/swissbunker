//! The `manifest.json` that makes a bunker disk describe itself.
//!
//! Spec §5: plug the disk into another machine and the dashboard must know what is on it
//! without scanning hundreds of gigabytes. Everything the Portable runtime needs to render
//! its opening screen lives here, in a file small enough to read instantly.
//!
//! The manifest is also the only channel through which the Forge tells the Reader things it
//! cannot infer — most importantly whether the documents were ordered by a real importance
//! signal, which changes what unranked results mean.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// How documents were ordered when the index was built.
///
/// This is surfaced rather than assumed because it changes the MEANING of unranked results
/// (spec §6.5). With a real signal they are "the most important documents containing this
/// word"; without one they are "some documents containing this word", and a UI that presents
/// them identically is overstating what it knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportanceSignal {
    /// Inbound link count — the classic proxy for encyclopedic relevance.
    InboundLinks,
    /// Aggregate pageviews.
    Pageviews,
    /// Source order preserved; the corpus was already curated.
    SourceOrder,
    /// No signal available. Unranked results are arbitrary, and the UI must say so.
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusEntry {
    pub id: String,
    pub name: String,
    pub language: String,
    pub documents: u64,
    pub index_bytes: u64,
    pub index_file: String,
    pub snapshot: String,
    pub importance_signal: ImportanceSignal,
    /// Empty when the source was imported by hand and never hashed.
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Bumped when the shape changes incompatibly, so an older Reader can refuse politely
    /// rather than misinterpret.
    pub version: u32,
    pub built_by: String,
    pub corpora: Vec<CorpusEntry>,
}

pub const MANIFEST_VERSION: u32 = 1;

impl Manifest {
    pub fn new() -> Self {
        Self {
            version: MANIFEST_VERSION,
            built_by: format!("swissbunkerd {}", env!("CARGO_PKG_VERSION")),
            corpora: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: CorpusEntry) {
        // Rebuilding a corpus replaces its entry rather than adding a second one: two entries
        // with the same id would make the dashboard show a corpus twice and disagree with
        // itself about its size.
        self.corpora.retain(|c| c.id != entry.id);
        self.corpora.push(entry);
        self.corpora.sort_by(|a, b| a.id.cmp(&b.id));
    }

    pub fn get(&self, id: &str) -> Option<&CorpusEntry> {
        self.corpora.iter().find(|c| c.id == id)
    }

    pub fn total_documents(&self) -> u64 {
        self.corpora.iter().map(|c| c.documents).sum()
    }

    pub fn total_index_bytes(&self) -> u64 {
        self.corpora.iter().map(|c| c.index_bytes).sum()
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest at {}", path.display()))?;
        let m: Manifest = serde_json::from_str(&text)
            .with_context(|| format!("parsing manifest at {}", path.display()))?;
        Ok(m)
    }

    /// Write atomically: a manifest truncated by an unplugged cable would leave a disk that
    /// describes itself as empty while holding hundreds of gigabytes.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, text.as_bytes())
            .with_context(|| format!("writing {}", tmp.display()))?;
        // rename is atomic within a filesystem, including exFAT: the manifest is either the
        // old one or the new one, never half of either.
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }
}

impl Default for Manifest {
    fn default() -> Self {
        Self::new()
    }
}
