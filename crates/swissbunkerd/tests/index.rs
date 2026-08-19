use rusqlite::Connection;
use swissbunkerd::index::{build_index, Document};
use swissbunkerd::journal::{JobId, Journal, Stage};

fn doc(title: &str, body: &str, importance: f64) -> Document {
    Document {
        title: title.into(),
        body: body.into(),
        source_path: format!("A/{}", title),
        importance,
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    journal: Journal,
    out: std::path::PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(&dir.path().join("journal.db")).unwrap();
    let out = dir.path().join("index.sqlite");
    Fixture {
        _dir: dir,
        journal,
        out,
    }
}

#[test]
fn builds_a_searchable_index() {
    let f = fixture();
    let docs = vec![
        doc(
            "Acqua potabile",
            "La potabilizzazione dell'acqua è un processo",
            1.0,
        ),
        doc(
            "Fotosintesi",
            "La fotosintesi clorofilliana avviene nelle piante",
            2.0,
        ),
    ];
    let stats = build_index(docs.into_iter(), &f.out, &f.journal, &JobId("t".into())).unwrap();
    assert_eq!(stats.documents, 2);

    let conn = Connection::open(&f.out).unwrap();
    let title: String = conn
        .query_row(
            "SELECT title FROM docs WHERE docs MATCH 'fotosintesi'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(title, "Fotosintesi");
}

#[test]
fn documents_are_stored_most_important_first() {
    // The requirement this whole module exists to satisfy (spec §6.5). FTS5 returns unranked
    // matches in rowid order, and phase 1 measured that roughly half of real searches take
    // the unranked path — so rowid order IS result order for half of everything.
    let f = fixture();
    let docs = vec![
        doc("Least", "comune parola comune", 0.1),
        doc("Most", "comune parola comune", 9.9),
        doc("Middle", "comune parola comune", 5.0),
    ];
    build_index(docs.into_iter(), &f.out, &f.journal, &JobId("t".into())).unwrap();

    let conn = Connection::open(&f.out).unwrap();
    let mut stmt = conn
        .prepare("SELECT title FROM docs WHERE docs MATCH 'comune'")
        .unwrap();
    let titles: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(titles, vec!["Most", "Middle", "Least"]);
}

#[test]
fn ties_do_not_lose_documents() {
    // Equal importance is the common case for corpora with no usable signal; it must degrade
    // to "some order" rather than to "some documents".
    let f = fixture();
    let docs: Vec<Document> = (0..50)
        .map(|i| doc(&format!("Doc {i}"), "identico contenuto", 1.0))
        .collect();
    let stats = build_index(docs.into_iter(), &f.out, &f.journal, &JobId("t".into())).unwrap();
    assert_eq!(stats.documents, 50);

    let conn = Connection::open(&f.out).unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM docs WHERE docs MATCH 'identico'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 50);
}

#[test]
fn the_schema_matches_what_the_reader_expects() {
    // The phase 1 reader queries `docs` with columns title and body, and calls
    // snippet(docs, 1, …) — the 1 meaning body is the second column. A schema change here
    // would break the browser side silently, so it is pinned by a test on this side.
    let f = fixture();
    build_index(
        vec![doc("T", "corpo", 1.0)].into_iter(),
        &f.out,
        &f.journal,
        &JobId("t".into()),
    )
    .unwrap();

    let conn = Connection::open(&f.out).unwrap();
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'docs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(sql.contains("fts5"), "docs is not an fts5 table: {sql}");
    assert!(sql.contains("title"), "missing title column: {sql}");
    assert!(sql.contains("body"), "missing body column: {sql}");

    // snippet() with column index 1 must resolve to body, exactly as the reader calls it.
    let snip: String = conn
        .query_row(
            "SELECT snippet(docs, 1, '<mark>', '</mark>', '…', 8) FROM docs WHERE docs MATCH 'corpo'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        snip.contains("<mark>corpo</mark>"),
        "unexpected snippet: {snip}"
    );
}

#[test]
fn source_path_survives_for_later_retrieval() {
    // The index keeps a pointer back into the original archive, so a future ZIM reader can
    // open the real document rather than only the indexed text.
    let f = fixture();
    build_index(
        vec![doc("Acqua", "testo", 1.0)].into_iter(),
        &f.out,
        &f.journal,
        &JobId("t".into()),
    )
    .unwrap();
    let conn = Connection::open(&f.out).unwrap();
    let path: String = conn
        .query_row(
            "SELECT source_path FROM docs WHERE docs MATCH 'testo'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(path, "A/Acqua");
}

#[test]
fn the_stage_is_marked_complete_in_the_journal() {
    let f = fixture();
    let job = JobId("t".into());
    build_index(
        vec![doc("T", "x", 1.0)].into_iter(),
        &f.out,
        &f.journal,
        &job,
    )
    .unwrap();
    assert!(f.journal.is_complete(&job, Stage::Index).unwrap());
}

#[test]
fn rebuilding_replaces_rather_than_appends() {
    // Running the pipeline twice must not double every document. Appending would be the
    // silent kind of wrong: searches still work, they just return everything twice.
    let f = fixture();
    let job = JobId("t".into());
    for _ in 0..2 {
        build_index(
            vec![doc("T", "unico", 1.0)].into_iter(),
            &f.out,
            &f.journal,
            &job,
        )
        .unwrap();
    }
    let conn = Connection::open(&f.out).unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM docs WHERE docs MATCH 'unico'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn an_empty_input_produces_a_valid_empty_index() {
    // A corpus that yields nothing is a bug upstream, but it must not leave a corrupt file
    // that fails confusingly later.
    let f = fixture();
    let stats = build_index(
        Vec::<Document>::new().into_iter(),
        &f.out,
        &f.journal,
        &JobId("t".into()),
    )
    .unwrap();
    assert_eq!(stats.documents, 0);
    let conn = Connection::open(&f.out).unwrap();
    let n: i64 = conn
        .query_row("SELECT count(*) FROM docs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn italian_accents_are_searchable() {
    let f = fixture();
    build_index(
        vec![doc("Città", "La città è perché così", 1.0)].into_iter(),
        &f.out,
        &f.journal,
        &JobId("t".into()),
    )
    .unwrap();
    let conn = Connection::open(&f.out).unwrap();
    let title: String = conn
        .query_row(
            "SELECT title FROM docs WHERE docs MATCH '\"perché\"'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(title, "Città");
}
