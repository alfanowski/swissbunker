use std::io::Write;
use swissbunkerd::import::{detect_format, import, import_jsonl, CorpusFormat};
use swissbunkerd::index::Document;
use swissbunkerd::journal::{JobId, Journal, Stage};
use swissbunkerd::manifest::ImportanceSignal;

struct Fixture {
    _dir: tempfile::TempDir,
    journal: Journal,
    dir: std::path::PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(&dir.path().join("journal.db")).unwrap();
    let path = dir.path().to_path_buf();
    Fixture {
        _dir: dir,
        journal,
        dir: path,
    }
}

fn write(f: &Fixture, name: &str, content: &str) -> std::path::PathBuf {
    let p = f.dir.join(name);
    let mut file = std::fs::File::create(&p).unwrap();
    file.write_all(content.as_bytes()).unwrap();
    p
}

fn collect(f: &Fixture, path: &std::path::Path) -> (Vec<Document>, u64, ImportanceSignal) {
    let mut docs = Vec::new();
    let stats = import_jsonl(path, &f.journal, &JobId("t".into()), |d| {
        docs.push(d);
        Ok(())
    })
    .unwrap();
    (docs, stats.skipped, stats.signal)
}

#[test]
fn reads_a_minimal_jsonl_corpus() {
    let f = fixture();
    let p = write(
        &f,
        "c.jsonl",
        "{\"title\":\"Acqua\",\"body\":\"La potabilizzazione\"}\n\
         {\"title\":\"Fuoco\",\"body\":\"L'accensione\"}\n",
    );
    let (docs, skipped, _) = collect(&f, &p);
    assert_eq!(docs.len(), 2);
    assert_eq!(skipped, 0);
    assert_eq!(docs[0].title, "Acqua");
}

#[test]
fn source_order_becomes_descending_importance() {
    // Spec §6.5: the first document in the file must end up first in the index, because
    // insertion order is result order for every search too broad to rank.
    let f = fixture();
    let p = write(
        &f,
        "c.jsonl",
        "{\"title\":\"First\",\"body\":\"x\"}\n\
         {\"title\":\"Second\",\"body\":\"x\"}\n\
         {\"title\":\"Third\",\"body\":\"x\"}\n",
    );
    let (docs, _, signal) = collect(&f, &p);
    assert!(docs[0].importance > docs[1].importance);
    assert!(docs[1].importance > docs[2].importance);
    assert_eq!(signal, ImportanceSignal::SourceOrder);
}

#[test]
fn an_explicit_importance_overrides_source_order() {
    let f = fixture();
    let p = write(
        &f,
        "c.jsonl",
        "{\"title\":\"Low\",\"body\":\"x\",\"importance\":1.0}\n\
         {\"title\":\"High\",\"body\":\"x\",\"importance\":9.0}\n",
    );
    let (docs, _, signal) = collect(&f, &p);
    assert_eq!(docs[0].importance, 1.0);
    assert_eq!(docs[1].importance, 9.0);
    // The manifest must record Explicit, not invent a provenance: the operator supplied
    // numbers and we do not know what they measure.
    assert_eq!(signal, ImportanceSignal::Explicit);
}

#[test]
fn a_malformed_line_is_skipped_not_fatal() {
    // One bad character in a million-line corpus must not throw away the other 999,999.
    let f = fixture();
    let p = write(
        &f,
        "c.jsonl",
        "{\"title\":\"Good\",\"body\":\"x\"}\n\
         this is not json at all\n\
         {\"title\":\"Also good\",\"body\":\"y\"}\n",
    );
    let (docs, skipped, _) = collect(&f, &p);
    assert_eq!(docs.len(), 2);
    assert_eq!(skipped, 1);
}

#[test]
fn a_truncated_last_line_is_skipped_not_fatal() {
    // The common shape of an interrupted export.
    let f = fixture();
    let p = write(
        &f,
        "c.jsonl",
        "{\"title\":\"Good\",\"body\":\"x\"}\n{\"title\":\"Trunc\",\"bo",
    );
    let (docs, skipped, _) = collect(&f, &p);
    assert_eq!(docs.len(), 1);
    assert_eq!(skipped, 1);
}

#[test]
fn blank_lines_are_ignored_without_counting_as_errors() {
    let f = fixture();
    let p = write(
        &f,
        "c.jsonl",
        "{\"title\":\"A\",\"body\":\"x\"}\n\n   \n{\"title\":\"B\",\"body\":\"y\"}\n",
    );
    let (docs, skipped, _) = collect(&f, &p);
    assert_eq!(docs.len(), 2);
    assert_eq!(skipped, 0);
}

#[test]
fn an_entirely_empty_document_is_skipped() {
    let f = fixture();
    let p = write(
        &f,
        "c.jsonl",
        "{\"title\":\"\",\"body\":\"\"}\n{\"title\":\"Real\",\"body\":\"x\"}\n",
    );
    let (docs, skipped, _) = collect(&f, &p);
    assert_eq!(docs.len(), 1);
    assert_eq!(skipped, 1);
}

#[test]
fn a_missing_source_path_gets_a_traceable_one() {
    // Without it there is no way back from a search result to where it came from.
    let f = fixture();
    let p = write(&f, "c.jsonl", "{\"title\":\"A\",\"body\":\"x\"}\n");
    let (docs, _, _) = collect(&f, &p);
    assert!(
        docs[0].source_path.contains("c.jsonl"),
        "{}",
        docs[0].source_path
    );
    assert!(
        docs[0].source_path.ends_with("#1"),
        "{}",
        docs[0].source_path
    );
}

#[test]
fn italian_text_survives_unchanged() {
    let f = fixture();
    let p = write(
        &f,
        "c.jsonl",
        "{\"title\":\"Città\",\"body\":\"Perché così è, e non può essere altrimenti\"}\n",
    );
    let (docs, _, _) = collect(&f, &p);
    assert_eq!(docs[0].title, "Città");
    assert!(docs[0].body.contains("Perché"));
}

#[test]
fn the_stage_is_marked_complete() {
    let f = fixture();
    let job = JobId("t".into());
    let p = write(&f, "c.jsonl", "{\"title\":\"A\",\"body\":\"x\"}\n");
    import_jsonl(&p, &f.journal, &job, |_| Ok(())).unwrap();
    assert!(f.journal.is_complete(&job, Stage::Extract).unwrap());
}

#[test]
fn format_is_detected_from_content_not_only_the_name() {
    // A file renamed by hand is common on a disk the operator fills themselves.
    let f = fixture();
    let p = write(&f, "corpus.txt", "{\"title\":\"A\",\"body\":\"x\"}\n");
    assert_eq!(detect_format(&p).unwrap(), CorpusFormat::Jsonl);
}

#[test]
fn a_zim_archive_is_recognised_and_refused_clearly() {
    // Saying "not yet, do this instead" beats failing halfway through with a parse error.
    let f = fixture();
    let p = f.dir.join("wiki.zim");
    // The ZIM magic number, little-endian.
    std::fs::write(&p, [0x5A, 0x49, 0x4D, 0x04, 0, 0, 0, 0]).unwrap();
    assert_eq!(detect_format(&p).unwrap(), CorpusFormat::Zim);

    let err = import(&p, &f.journal, &JobId("t".into()), |_| Ok(())).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("ZIM"), "{msg}");
    assert!(
        msg.contains("JSONL"),
        "error should say what to do instead: {msg}"
    );
}

#[test]
fn an_unrecognisable_file_is_refused_with_the_supported_list() {
    let f = fixture();
    let p = write(&f, "mystery.bin", "\x00\x01\x02 not anything we know");
    let err = import(&p, &f.journal, &JobId("t".into()), |_| Ok(())).unwrap_err();
    assert!(err.to_string().contains("JSONL"), "{err}");
}
