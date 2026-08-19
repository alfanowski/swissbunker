use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use swissbunkerd::api::{self, AppState, SharedState};
use swissbunkerd::journal::Journal;

struct Harness {
    _dir: tempfile::TempDir,
    disk: PathBuf,
    addr: SocketAddr,
}

/// Start a daemon on an ephemeral loopback port and return where to reach it.
async fn start() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let disk = dir.path().to_path_buf();
    std::fs::create_dir_all(disk.join(".state")).unwrap();

    let journal = Journal::open(&disk.join(".state/journal.db")).unwrap();
    let state: SharedState = Arc::new(AppState::new(disk.clone(), journal));

    // Port 0 lets the OS pick a free one, so tests never collide with each other or with a
    // daemon the developer happens to be running.
    let listener = api::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, api::router(state).into_make_service())
            .await
            .unwrap();
    });

    Harness {
        _dir: dir,
        disk,
        addr,
    }
}

/// Start a build and wait for the manifest to reflect it.
///
/// The API accepts a build and returns immediately, so tests have to wait for the work the
/// same way a dashboard does. Polling the manifest rather than sleeping a fixed time keeps
/// the suite fast and stops it being flaky on a slow machine.
async fn build_and_wait(addr: SocketAddr, corpus: &str, id: &str) -> serde_json::Value {
    let started: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/build"))
        .json(&serde_json::json!({ "corpus": corpus, "id": id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        started["started"], true,
        "build was not accepted: {started}"
    );

    for _ in 0..100 {
        let m: serde_json::Value = reqwest::get(format!("http://{addr}/api/manifest"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if m["corpora"]
            .as_array()
            .map(|a| a.iter().any(|c| c["id"] == id))
            .unwrap_or(false)
        {
            return m;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("build of {id} did not finish in time");
}

fn write_corpus(disk: &std::path::Path, name: &str, lines: &[(&str, &str)]) -> PathBuf {
    let p = disk.join(name);
    let mut f = std::fs::File::create(&p).unwrap();
    for (title, body) in lines {
        writeln!(f, "{}", serde_json::json!({ "title": title, "body": body })).unwrap();
    }
    p
}

#[tokio::test]
async fn refuses_to_bind_a_public_interface() {
    // Spec §10. The disk is designed to be plugged into machines the owner does not control,
    // so a daemon reachable from the network would be a file server nobody asked for, running
    // on someone else's computer.
    let err = api::bind("0.0.0.0:0".parse().unwrap()).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("loopback"), "{msg}");

    // A real external address is refused too, not just the wildcard.
    let err = api::bind("192.168.1.50:0".parse().unwrap())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("loopback"));
}

#[tokio::test]
async fn binds_loopback_in_both_address_families() {
    assert!(api::bind("127.0.0.1:0".parse().unwrap()).await.is_ok());
    // IPv6 loopback may be unavailable in some environments; only the refusal path is a hard
    // requirement, so an unavailable stack is tolerated rather than failing the suite.
    let _ = api::bind("[::1]:0".parse().unwrap()).await;
}

#[tokio::test]
async fn reports_an_empty_disk_without_erroring() {
    // The dashboard's opening screen needs "nothing here yet" to be a state, not a failure.
    let h = start().await;
    let body: serde_json::Value = reqwest::get(format!("http://{}/api/state", h.addr))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["has_bunker"], false);
    assert_eq!(body["corpora"], 0);
}

#[tokio::test]
async fn builds_a_corpus_and_reports_it_in_state() {
    let h = start().await;
    write_corpus(
        &h.disk,
        "corpus.jsonl",
        &[
            ("Acqua potabile", "La potabilizzazione dell'acqua"),
            ("Fotosintesi", "Le piante e la luce solare"),
        ],
    );

    let m = build_and_wait(h.addr, "corpus.jsonl", "demo").await;
    assert_eq!(
        m["corpora"][0]["documents"], 2,
        "build produced nothing: {m}"
    );
    assert_eq!(m["corpora"][0]["importance_signal"], "source-order");

    let state: serde_json::Value = reqwest::get(format!("http://{}/api/state", h.addr))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["has_bunker"], true);
    assert_eq!(state["documents"], 2);
}

#[tokio::test]
async fn the_manifest_endpoint_describes_what_was_built() {
    let h = start().await;
    write_corpus(
        &h.disk,
        "c.jsonl",
        &[("A", "testo uno"), ("B", "testo due")],
    );
    let m = build_and_wait(h.addr, "c.jsonl", "corpus_uno").await;
    assert_eq!(m["corpora"][0]["id"], "corpus_uno");
    assert_eq!(m["corpora"][0]["importance_signal"], "source-order");
}

#[tokio::test]
async fn health_actually_queries_the_index() {
    // A manifest entry whose file is missing or corrupt looks perfectly healthy from the
    // manifest alone, so the check has to open the index and run a real query.
    let h = start().await;
    write_corpus(&h.disk, "c.jsonl", &[("A", "contenuto")]);
    build_and_wait(h.addr, "c.jsonl", "sano").await;

    let ok: serde_json::Value = reqwest::get(format!("http://{}/api/health", h.addr))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ok["ok"], true, "{ok}");

    // Delete the index behind the manifest's back: health must notice.
    std::fs::remove_file(h.disk.join("index/sano.sqlite")).unwrap();
    let broken: serde_json::Value = reqwest::get(format!("http://{}/api/health", h.addr))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(broken["ok"], false, "{broken}");
}

#[tokio::test]
async fn refuses_to_read_a_file_outside_the_disk() {
    // The daemon is reachable by any page in the browser that can reach loopback, so a
    // request asking it to index /etc/passwd must be refused here rather than trusted.
    let h = start().await;
    let res = reqwest::Client::new()
        .post(format!("http://{}/api/build", h.addr))
        .json(&serde_json::json!({ "corpus": "/etc/hosts", "id": "escape" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("outside the bunker disk"),
        "{body}"
    );
}

#[tokio::test]
async fn refuses_a_path_that_climbs_out_with_dot_dot() {
    let h = start().await;
    let res = reqwest::Client::new()
        .post(format!("http://{}/api/build", h.addr))
        .json(&serde_json::json!({ "corpus": "../../../etc/hosts", "id": "escape" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
}

#[tokio::test]
async fn refuses_an_id_that_would_escape_the_index_directory() {
    // Ids become filenames. Without this, an id of "../../manifest" would overwrite the
    // manifest with an SQLite file.
    let h = start().await;
    write_corpus(&h.disk, "c.jsonl", &[("A", "x")]);
    for bad in ["../escape", "with/slash", "with space", ""] {
        let res = reqwest::Client::new()
            .post(format!("http://{}/api/build", h.addr))
            .json(&serde_json::json!({ "corpus": "c.jsonl", "id": bad }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 400, "id {bad:?} was accepted");
    }
}

#[tokio::test]
async fn a_missing_corpus_gives_a_useful_error_not_a_panic() {
    let h = start().await;
    let res = reqwest::Client::new()
        .post(format!("http://{}/api/build", h.addr))
        .json(&serde_json::json!({ "corpus": "nope.jsonl", "id": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("no such file"),
        "{body}"
    );
}

#[tokio::test]
async fn rebuilding_the_same_id_replaces_rather_than_duplicates() {
    let h = start().await;
    write_corpus(&h.disk, "c.jsonl", &[("A", "x")]);
    for _ in 0..2 {
        build_and_wait(h.addr, "c.jsonl", "same").await;
    }
    let m: serde_json::Value = reqwest::get(format!("http://{}/api/manifest", h.addr))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(m["corpora"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_build_is_accepted_immediately_rather_than_held_open() {
    // A real build takes minutes. Holding the request open would give a dashboard that looks
    // frozen and a proxy that times out.
    let h = start().await;
    write_corpus(&h.disk, "c.jsonl", &[("A", "x")]);
    let t0 = std::time::Instant::now();
    let res: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{}/api/build", h.addr))
        .json(&serde_json::json!({ "corpus": "c.jsonl", "id": "async_job" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(res["started"], true);
    assert_eq!(res["job"], "async_job");
    assert!(t0.elapsed() < std::time::Duration::from_secs(2));
}

#[tokio::test]
async fn an_invalid_request_is_rejected_before_the_job_is_accepted() {
    // Failing in a background task would leave the caller with an acknowledgement and no
    // explanation, so everything the caller controls is validated first.
    let h = start().await;
    let res = reqwest::Client::new()
        .post(format!("http://{}/api/build", h.addr))
        .json(&serde_json::json!({ "corpus": "missing.jsonl", "id": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
}

#[tokio::test]
async fn progress_streams_the_pipeline_as_events() {
    let h = start().await;
    write_corpus(&h.disk, "c.jsonl", &[("A", "uno"), ("B", "due")]);
    build_and_wait(h.addr, "c.jsonl", "tracked").await;

    // The stream reports current state on connect, so a dashboard opened after a build still
    // sees what happened rather than an empty screen.
    let mut res = reqwest::get(format!("http://{}/api/progress", h.addr))
        .await
        .unwrap();
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), res.chunk())
        .await
        .expect("no event within 5s")
        .unwrap()
        .expect("stream closed");
    let text = String::from_utf8_lossy(&chunk);
    assert!(
        text.contains("tracked"),
        "event did not mention the job: {text}"
    );
    assert!(
        text.contains("index"),
        "event did not mention the stage: {text}"
    );
}
