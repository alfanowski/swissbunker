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
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::import::import;
use crate::index::{build_index, Document};
use crate::journal::{JobId, Journal};
use crate::manifest::{CorpusEntry, Manifest};

/// Everything a request handler is allowed to touch.
pub struct AppState {
    pub disk: PathBuf,
    pub journal: Journal,
    /// One build at a time.
    ///
    /// Not a queue: two builds would contend for the same journal and, on exFAT, for a
    /// database that must be opened with EXCLUSIVE locking. Refusing the second request with
    /// a clear message beats a queue nobody asked for or a deadlock nobody expected.
    pub busy: std::sync::atomic::AtomicBool,
}

impl AppState {
    pub fn new(disk: PathBuf, journal: Journal) -> Self {
        Self {
            disk,
            journal,
            busy: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// How often the progress stream samples the journal.
///
/// Progress is READ from the journal rather than pushed through a channel, because the
/// journal is already the source of truth and already survives a crash. A channel would
/// duplicate that state and then have to be kept in agreement with it — and the two would
/// disagree exactly when something went wrong, which is when progress matters most.
const PROGRESS_POLL: Duration = Duration::from_millis(250);

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

/// The answer to "start a build": an acknowledgement, not a result.
///
/// A real build takes minutes to hours, so holding the request open until it finishes would
/// mean a dashboard that appears frozen and a proxy that times out. The caller follows
/// /api/progress from here.
#[derive(Debug, Serialize)]
pub struct BuildStarted {
    pub job: String,
    pub started: bool,
}

/// An error that is safe to show a user: it says what went wrong and, where possible, what to
/// do about it. Internal detail stays in the daemon's own output.
pub struct ApiError(pub anyhow::Error);

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

/// Build the router, optionally serving a dashboard directory alongside the API.
///
/// The dashboard is served from the disk rather than embedded in the binary, so the same
/// files back both modes: Connected reaches them over http, Portable opens them directly
/// from `file://`. Embedding would mean two copies that drift.
pub fn router_with_app(state: SharedState, app_dir: Option<PathBuf>) -> Router {
    let router = router(state);
    match app_dir {
        Some(dir) if dir.is_dir() => router.fallback_service(
            tower_http::services::ServeDir::new(dir).append_index_html_on_directories(true),
        ),
        // No dashboard on the disk yet is not an error: the API still works, and the CLI is a
        // complete way to use the daemon.
        _ => router,
    }
}

/// Build the router. Separated from serving so tests can drive it without a socket.
pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/api/state", get(get_state))
        .route("/api/manifest", get(get_manifest))
        .route("/api/build", post(post_build))
        .route("/api/health", get(get_health))
        .route("/api/progress", get(get_progress))
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

/// Stream job progress as server-sent events.
///
/// Emits one event per poll containing every stage of every job, so a dashboard can render
/// the whole pipeline from a single message rather than stitching together deltas.
async fn get_progress(
    State(s): State<SharedState>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut previous = String::new();
        loop {
            let payload = match s.journal.all_progress() {
                Ok(rows) => {
                    let items: Vec<_> = rows
                        .iter()
                        .map(|p| serde_json::json!({
                            "job": p.job.0,
                            "stage": p.stage.as_str(),
                            "position": p.position,
                            "total": p.total,
                            "done": p.done,
                            "error": p.error,
                        }))
                        .collect();
                    serde_json::json!({ "jobs": items }).to_string()
                }
                Err(e) => serde_json::json!({ "error": format!("{e:#}") }).to_string(),
            };

            // Only send when something changed. A dashboard left open for an hour should not
            // accumulate fourteen thousand identical messages.
            if payload != previous {
                previous = payload.clone();
                yield Ok(Event::default().data(payload));
            }
            tokio::time::sleep(PROGRESS_POLL).await;
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn post_build(
    State(s): State<SharedState>,
    Json(req): Json<BuildRequest>,
) -> Result<Json<BuildStarted>, ApiError> {
    use std::sync::atomic::Ordering;

    // Validate everything the caller controls BEFORE accepting the job. A request that is
    // going to fail should fail now, with an explanation, rather than in a background task
    // whose only trace is a line in the progress stream.
    let corpus = resolve_corpus(&s.disk, &req.corpus)?;
    let id = sanitise_id(&req.id)?;
    let name = req.name.clone().unwrap_or_else(|| id.clone());
    let language = req.language.clone().unwrap_or_else(|| "it".to_string());

    if s.busy.swap(true, Ordering::SeqCst) {
        return Err(ApiError(anyhow::anyhow!(
            "a build is already running; wait for it to finish or restart the daemon"
        )));
    }

    let state = s.clone();
    let job_id = id.clone();
    // spawn_blocking, not spawn: importing and indexing are synchronous CPU and disk work,
    // and running them on an async worker would stall every other request including the
    // progress stream that exists to report on them.
    tokio::task::spawn_blocking(move || {
        let result = run_build(&state, &corpus, &job_id, &name, &language);
        if let Err(e) = result {
            // The failure is recorded where the dashboard already looks, rather than only in
            // the daemon's own output where nobody would see it.
            let _ = state.journal.failed(
                &JobId(job_id.clone()),
                crate::journal::Stage::Index,
                &format!("{e:#}"),
            );
        }
        state.busy.store(false, Ordering::SeqCst);
    });

    Ok(Json(BuildStarted {
        job: id,
        started: true,
    }))
}

/// The whole pipeline for one corpus, synchronous by nature.
fn run_build(s: &AppState, corpus: &Path, id: &str, name: &str, language: &str) -> Result<()> {
    std::fs::create_dir_all(index_dir(&s.disk))?;
    let job = JobId(id.to_string());

    let mut docs: Vec<Document> = Vec::new();
    let stats = import(corpus, &s.journal, &job, |d| {
        docs.push(d);
        Ok(())
    })?;
    if stats.documents == 0 {
        bail!("no usable documents in {}", corpus.display());
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
        id: id.to_string(),
        name: name.to_string(),
        language: language.to_string(),
        documents: built.documents,
        index_bytes: built.bytes,
        index_file: format!("index/{id}.sqlite"),
        snapshot: String::new(),
        importance_signal: stats.signal,
        sha256: String::new(),
    });
    manifest.save(&mpath)?;
    Ok(())
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

/// Where the dashboard lives on a bunker disk, per spec §5.
pub fn app_dir(disk: &Path) -> PathBuf {
    disk.join("app")
}

/// Run the daemon until the process ends.
pub async fn serve(disk: PathBuf, addr: SocketAddr) -> Result<()> {
    serve_with_app(disk.clone(), addr, Some(app_dir(&disk))).await
}

/// Run the daemon, serving a dashboard from `app` if it exists.
pub async fn serve_with_app(disk: PathBuf, addr: SocketAddr, app: Option<PathBuf>) -> Result<()> {
    if !disk.exists() {
        bail!("no such disk: {}", disk.display());
    }
    std::fs::create_dir_all(state_dir(&disk))?;
    let journal = Journal::open(&state_dir(&disk).join("journal.db"))?;
    let state: SharedState = Arc::new(AppState::new(disk, journal));

    let listener = bind(addr).await?;
    let local = listener.local_addr()?;
    match &app {
        Some(d) if d.is_dir() => println!("swissbunkerd serving {} on http://{local}", d.display()),
        _ => println!("swissbunkerd listening on http://{local} (API only, no dashboard on disk)"),
    }
    axum::serve(listener, router_with_app(state, app).into_make_service())
        .await
        .context("serving")?;
    Ok(())
}
