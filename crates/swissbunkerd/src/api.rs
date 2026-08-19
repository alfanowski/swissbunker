//! The local HTTP API, and the server that hosts it.
//!
//! This is what turns the Forge from a command-line tool into the dashboard the product
//! promises. It runs from the disk, binds to loopback only, and speaks JSON to a page served
//! from the same origin.
//!
//! Everything here is deliberately thin: the real work lives in `import`, `index` and
//! `manifest`, and this layer only decides what the browser is allowed to ask for.

use anyhow::{bail, Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::import::import;
use crate::index::{build_index, Document};
use crate::journal::{JobId, Journal};
use crate::manifest::{CorpusEntry, Manifest};

/// Everything a request handler is allowed to touch.
pub struct AppState {
    pub disk: PathBuf,
    pub journal: Journal,
}

pub type SharedState = Arc<AppState>;

#[derive(Debug, Serialize)]
pub struct DiskState {
    pub disk: String,
    pub has_bunker: bool,
    pub corpora: usize,
    pub documents: u64,
    pub index_bytes: u64,
}

#[derive(Debug, Deserialize)]
pub struct BuildRequest {
    /// Path to a corpus file, relative to the disk or absolute.
    pub corpus: String,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BuildResponse {
    pub id: String,
    pub documents: u64,
    pub skipped: u64,
    pub index_bytes: u64,
    pub build_secs: f64,
    pub importance_signal: String,
}

/// An error that is safe to show a user: it says what went wrong and, where possible, what to
/// do about it. Internal detail stays in the daemon's own output.
pub struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let message = format!("{:#}", self.0);
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": message })),
        )
            .into_response()
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

pub fn manifest_path(disk: &Path) -> PathBuf {
    disk.join("manifest.json")
}
pub fn index_dir(disk: &Path) -> PathBuf {
    disk.join("index")
}
pub fn state_dir(disk: &Path) -> PathBuf {
    disk.join(".state")
}

/// Build the router. Separated from serving so tests can drive it without a socket.
pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/api/state", get(get_state))
        .route("/api/manifest", get(get_manifest))
        .route("/api/build", post(post_build))
        .route("/api/health", get(get_health))
        .with_state(state)
}

async fn get_state(State(s): State<SharedState>) -> Result<Json<DiskState>, ApiError> {
    let mpath = manifest_path(&s.disk);
    if !mpath.exists() {
        return Ok(Json(DiskState {
            disk: s.disk.display().to_string(),
            has_bunker: false,
            corpora: 0,
            documents: 0,
            index_bytes: 0,
        }));
    }
    let m = Manifest::load(&mpath)?;
    Ok(Json(DiskState {
        disk: s.disk.display().to_string(),
        has_bunker: true,
        corpora: m.corpora.len(),
        documents: m.total_documents(),
        index_bytes: m.total_index_bytes(),
    }))
}

async fn get_manifest(State(s): State<SharedState>) -> Result<Json<Manifest>, ApiError> {
    let mpath = manifest_path(&s.disk);
    if !mpath.exists() {
        // An empty manifest rather than a 404: the dashboard's opening screen wants to render
        // "no content yet", and a missing file is that state, not an error.
        return Ok(Json(Manifest::new()));
    }
    Ok(Json(Manifest::load(&mpath)?))
}

/// A tiny self-check, per spec §9.3 screen 5: does this bunker actually answer a query?
async fn get_health(State(s): State<SharedState>) -> Result<Json<serde_json::Value>, ApiError> {
    let mpath = manifest_path(&s.disk);
    if !mpath.exists() {
        return Ok(Json(
            serde_json::json!({ "ok": false, "reason": "no bunker on this disk yet" }),
        ));
    }
    let m = Manifest::load(&mpath)?;
    let mut checks = Vec::new();
    for c in &m.corpora {
        let path = s.disk.join(&c.index_file);
        // Opening and querying each index is the only honest health check: a manifest entry
        // whose file is missing or corrupt looks perfectly healthy from the manifest alone.
        let result = crate::api::probe_index(&path);
        checks.push(serde_json::json!({
            "id": c.id,
            "ok": result.is_ok(),
            "detail": match result {
                Ok(n) => format!("{n} documents, searchable"),
                Err(e) => format!("{e:#}"),
            }
        }));
    }
    let ok = checks.iter().all(|c| c["ok"] == serde_json::json!(true));
    Ok(Json(serde_json::json!({ "ok": ok, "corpora": checks })))
}

/// Open an index and run a real query against it.
pub fn probe_index(path: &Path) -> Result<u64> {
    if !path.exists() {
        bail!("index file missing: {}", path.display());
    }
    let conn =
        rusqlite::Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
    let n: i64 = conn
        .query_row("SELECT count(*) FROM docs", [], |r| r.get(0))
        .context("counting documents")?;
    // A count alone would pass on an index whose FTS tables are broken, so a real MATCH runs
    // too. The term is irrelevant; that the query executes is the point.
    conn.query_row(
        "SELECT count(*) FROM docs WHERE docs MATCH ?",
        ["zzzprobezzz"],
        |r| r.get::<_, i64>(0),
    )
    .context("running a full-text query")?;
    Ok(n as u64)
}

async fn post_build(
    State(s): State<SharedState>,
    Json(req): Json<BuildRequest>,
) -> Result<Json<BuildResponse>, ApiError> {
    let corpus = resolve_corpus(&s.disk, &req.corpus)?;
    let id = sanitise_id(&req.id)?;

    std::fs::create_dir_all(index_dir(&s.disk))?;
    let job = JobId(id.clone());

    // Synchronous, and that is a known limitation rather than an oversight: a real build takes
    // minutes to hours and must stream progress instead of holding a request open. The
    // journal already records everything a progress endpoint would need; wiring it to
    // server-sent events is the next task, and doing it now would mean designing the event
    // shape before there is a UI asking for it.
    let mut docs: Vec<Document> = Vec::new();
    let stats = import(&corpus, &s.journal, &job, |d| {
        docs.push(d);
        Ok(())
    })?;
    if stats.documents == 0 {
        bail_api("no usable documents in that file")?;
    }

    let out = index_dir(&s.disk).join(format!("{id}.sqlite"));
    let built = build_index(docs.into_iter(), &out, &s.journal, &job)?;

    let mpath = manifest_path(&s.disk);
    let mut manifest = if mpath.exists() {
        Manifest::load(&mpath)?
    } else {
        Manifest::new()
    };
    manifest.add(CorpusEntry {
        id: id.clone(),
        name: req.name.unwrap_or_else(|| id.clone()),
        language: req.language.unwrap_or_else(|| "it".to_string()),
        documents: built.documents,
        index_bytes: built.bytes,
        index_file: format!("index/{id}.sqlite"),
        snapshot: String::new(),
        importance_signal: stats.signal,
        sha256: String::new(),
    });
    manifest.save(&mpath)?;

    Ok(Json(BuildResponse {
        id,
        documents: built.documents,
        skipped: stats.skipped,
        index_bytes: built.bytes,
        build_secs: built.build_secs,
        importance_signal: format!("{:?}", stats.signal),
    }))
}

fn bail_api(msg: &str) -> Result<()> {
    bail!("{msg}")
}

/// Resolve a corpus path, refusing anything that escapes the disk.
///
/// The daemon is reachable by any page in the browser that can reach loopback, so a request
/// asking it to read `/etc/passwd` must be refused here rather than trusted.
fn resolve_corpus(disk: &Path, requested: &str) -> Result<PathBuf> {
    let candidate = if Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        disk.join(requested)
    };

    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("no such file: {requested}"))?;
    let disk_canonical = disk
        .canonicalize()
        .with_context(|| format!("no such disk: {}", disk.display()))?;

    if !canonical.starts_with(&disk_canonical) {
        bail!(
            "refusing to read {} — it is outside the bunker disk",
            canonical.display()
        );
    }
    Ok(canonical)
}

/// Corpus ids become filenames, so they are constrained rather than trusted.
fn sanitise_id(id: &str) -> Result<String> {
    let clean: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if clean.is_empty() {
        bail!("id must contain letters, digits, underscore or hyphen");
    }
    if clean != id {
        bail!("id may only contain letters, digits, underscore and hyphen: got {id:?}");
    }
    Ok(clean)
}

/// Bind an address, refusing anything that is not loopback.
///
/// Spec §10. A bunker daemon reachable from the network is a file server nobody asked for, on
/// a machine that is not the owner's — and the disk is designed to be plugged into machines
/// the owner does not control.
pub async fn bind(addr: SocketAddr) -> Result<tokio::net::TcpListener> {
    let ip = addr.ip();
    let is_loopback = match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    };
    if !is_loopback {
        bail!(
            "refusing to bind {addr}: the daemon listens on loopback only. \
             Use 127.0.0.1 or [::1]."
        );
    }
    tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))
}

/// Run the daemon until the process ends.
pub async fn serve(disk: PathBuf, addr: SocketAddr) -> Result<()> {
    if !disk.exists() {
        bail!("no such disk: {}", disk.display());
    }
    std::fs::create_dir_all(state_dir(&disk))?;
    let journal = Journal::open(&state_dir(&disk).join("journal.db"))?;
    let state: SharedState = Arc::new(AppState { disk, journal });

    let listener = bind(addr).await?;
    let local = listener.local_addr()?;
    println!("swissbunkerd listening on http://{local}");
    axum::serve(listener, router(state).into_make_service())
        .await
        .context("serving")?;
    Ok(())
}
