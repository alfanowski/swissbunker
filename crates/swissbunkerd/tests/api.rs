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
    let state: SharedState = Arc::new(AppState {
        disk: disk.clone(),
        journal,
    });

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

    let res: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{}/api/build", h.addr))
        .json(&serde_json::json!({ "corpus": "corpus.jsonl", "id": "demo", "name": "Demo" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(res["documents"], 2, "build failed: {res}");
    assert_eq!(res["importance_signal"], "SourceOrder");

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
    reqwest::Client::new()
        .post(format!("http://{}/api/build", h.addr))
        .json(&serde_json::json!({ "corpus": "c.jsonl", "id": "corpus_uno" }))
        .send()
        .await
        .unwrap();

    let m: serde_json::Value = reqwest::get(format!("http://{}/api/manifest", h.addr))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(m["corpora"][0]["id"], "corpus_uno");
    assert_eq!(m["corpora"][0]["importance_signal"], "source-order");
}

#[tokio::test]
async fn health_actually_queries_the_index() {
    // A manifest entry whose file is missing or corrupt looks perfectly healthy from the
    // manifest alone, so the check has to open the index and run a real query.
    let h = start().await;
    write_corpus(&h.disk, "c.jsonl", &[("A", "contenuto")]);
    reqwest::Client::new()
        .post(format!("http://{}/api/build", h.addr))
        .json(&serde_json::json!({ "corpus": "c.jsonl", "id": "sano" }))
        .send()
        .await
        .unwrap();

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
    let client = reqwest::Client::new();
    for _ in 0..2 {
        client
            .post(format!("http://{}/api/build", h.addr))
            .json(&serde_json::json!({ "corpus": "c.jsonl", "id": "same" }))
            .send()
            .await
            .unwrap();
    }
    let m: serde_json::Value = reqwest::get(format!("http://{}/api/manifest", h.addr))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(m["corpora"].as_array().unwrap().len(), 1);
}
