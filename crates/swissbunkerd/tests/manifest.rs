use swissbunkerd::manifest::{CorpusEntry, ImportanceSignal, Manifest, MANIFEST_VERSION};

fn entry(id: &str, docs: u64) -> CorpusEntry {
    CorpusEntry {
        id: id.into(),
        name: format!("Corpus {id}"),
        language: "it".into(),
        documents: docs,
        index_bytes: docs * 1000,
        index_file: format!("index/{id}.sqlite"),
        snapshot: "2026-08".into(),
        importance_signal: ImportanceSignal::InboundLinks,
        sha256: String::new(),
    }
}

#[test]
fn a_new_manifest_declares_its_version() {
    assert_eq!(Manifest::new().version, MANIFEST_VERSION);
}

#[test]
fn round_trips_through_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    let mut m = Manifest::new();
    m.add(entry("wikipedia_it", 1_900_000));
    m.save(&path).unwrap();

    let back = Manifest::load(&path).unwrap();
    assert_eq!(back.corpora.len(), 1);
    assert_eq!(back.get("wikipedia_it").unwrap().documents, 1_900_000);
    assert_eq!(back.version, MANIFEST_VERSION);
}

#[test]
fn rebuilding_a_corpus_replaces_its_entry() {
    // Two entries with the same id would make the dashboard list a corpus twice and disagree
    // with itself about the disk's contents.
    let mut m = Manifest::new();
    m.add(entry("wikipedia_it", 100));
    m.add(entry("wikipedia_it", 200));
    assert_eq!(m.corpora.len(), 1);
    assert_eq!(m.get("wikipedia_it").unwrap().documents, 200);
}

#[test]
fn totals_add_up_across_corpora() {
    let mut m = Manifest::new();
    m.add(entry("a", 10));
    m.add(entry("b", 25));
    assert_eq!(m.total_documents(), 35);
    assert_eq!(m.total_index_bytes(), 35_000);
}

#[test]
fn the_importance_signal_is_recorded() {
    // It changes what unranked results MEAN, so the Reader has to be told rather than assume.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    let mut m = Manifest::new();
    let mut e = entry("x", 1);
    e.importance_signal = ImportanceSignal::None;
    m.add(e);
    m.save(&path).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("\"none\""),
        "signal not serialised readably: {text}"
    );
    assert_eq!(
        Manifest::load(&path)
            .unwrap()
            .get("x")
            .unwrap()
            .importance_signal,
        ImportanceSignal::None
    );
}

#[test]
fn saving_replaces_atomically_and_leaves_no_temp_file() {
    // A manifest truncated by an unplugged cable would leave a disk describing itself as
    // empty while holding hundreds of gigabytes.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    let mut m = Manifest::new();
    m.add(entry("a", 1));
    m.save(&path).unwrap();
    m.add(entry("b", 2));
    m.save(&path).unwrap();

    assert_eq!(Manifest::load(&path).unwrap().corpora.len(), 2);
    assert!(
        !path.with_extension("json.tmp").exists(),
        "temp file left behind"
    );
}

#[test]
fn corpora_are_listed_in_a_stable_order() {
    // The dashboard renders this list; an order that changes between builds would make the
    // UI shuffle for no reason the user can see.
    let mut m = Manifest::new();
    m.add(entry("zulu", 1));
    m.add(entry("alpha", 1));
    m.add(entry("mike", 1));
    let ids: Vec<&str> = m.corpora.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["alpha", "mike", "zulu"]);
}

#[test]
fn a_manifest_from_the_future_still_parses_its_known_fields() {
    // An older daemon meeting a newer disk should read what it understands rather than fail:
    // the version field exists so it can refuse deliberately, not accidentally.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    std::fs::write(
        &path,
        r#"{"version":99,"built_by":"future","corpora":[
             {"id":"x","name":"X","language":"it","documents":5,"index_bytes":10,
              "index_file":"index/x.sqlite","snapshot":"2027-01",
              "importance_signal":"inbound-links","sha256":""}]}"#,
    )
    .unwrap();
    let m = Manifest::load(&path).unwrap();
    assert_eq!(m.version, 99);
    assert_eq!(m.get("x").unwrap().documents, 5);
}
