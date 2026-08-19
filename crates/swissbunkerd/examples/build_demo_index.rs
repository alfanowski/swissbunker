//! Build a small demo index, for verifying that the Forge and the browser Reader agree.
//!
//! The two halves of this product are written in different languages and tested separately.
//! The only thing that proves they are one product is an index built by one and opened by the
//! other, which is what this example exists to produce.
//!
//! Documents carry deliberately different importance values so the browser side can check
//! that insertion order survived — the property from spec §6.5 that decides result order for
//! roughly half of real searches.
//!
//! Usage: cargo run -p swissbunkerd --example build_demo_index -- <out.sqlite>

use anyhow::Result;
use swissbunkerd::index::{build_index, Document};
use swissbunkerd::journal::{JobId, Journal};

/// (title, body, importance). Importance descends deliberately, and every document shares the
/// word "bunker" so one broad query can reveal the whole ordering at once.
const DEMO: &[(&str, &str, f64)] = &[
    (
        "Acqua potabile",
        "La potabilizzazione dell'acqua è il processo che rende l'acqua sicura da bere. \
         Nel bunker è la prima cosa da sapere. Filtrazione, bollitura e disinfezione.",
        100.0,
    ),
    (
        "Fotosintesi clorofilliana",
        "La fotosintesi clorofilliana è il processo con cui le piante convertono la luce \
         solare in energia chimica. Argomento di bunker e di biologia.",
        90.0,
    ),
    (
        "Penicillina",
        "La penicillina è stato il primo antibiotico scoperto, da Alexander Fleming nel 1928. \
         Un bunker medico non può prescinderne.",
        80.0,
    ),
    (
        "Battaglia di Lepanto",
        "La battaglia di Lepanto del 1571 vide la Lega Santa contrapporsi alla flotta ottomana. \
         Storia navale, non da bunker, ma qui per completezza.",
        70.0,
    ),
    (
        "Terremoto",
        "Un terremoto è un rapido rilascio di energia nella crosta terrestre. La scala Richter \
         ne misura la magnitudo. Sapere del bunker utile in caso di sisma.",
        60.0,
    ),
    (
        "Città di provincia",
        "Una città di provincia italiana qualsiasi, presente per riempire l'indice. \
         La parola bunker compare anche qui, con importanza bassa.",
        10.0,
    ),
    (
        "Nota marginale",
        "Documento di scarsissima rilevanza, ultimo per importanza. Contiene bunker \
         solo per finire in fondo ai risultati non ordinati.",
        1.0,
    ),
];

fn main() -> Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/demo-index.sqlite".to_string());
    let out = std::path::PathBuf::from(out);

    let journal_path = out.with_extension("journal.db");
    let _ = std::fs::remove_file(&journal_path);
    let journal = Journal::open(&journal_path)?;

    // Shuffled on the way in, so passing the ordering test cannot be an accident of the
    // order they happen to be written in this file.
    let mut docs: Vec<Document> = DEMO
        .iter()
        .map(|(title, body, importance)| Document {
            title: (*title).into(),
            body: (*body).into(),
            source_path: format!("A/{title}"),
            importance: *importance,
        })
        .collect();
    docs.reverse();

    let stats = build_index(docs.into_iter(), &out, &journal, &JobId("demo".into()))?;
    println!(
        "built {} — {} documents, {} KB, {:.2}s",
        out.display(),
        stats.documents,
        stats.bytes / 1024,
        stats.build_secs
    );
    println!("expected order for a search of \"bunker\":");
    for (title, _, imp) in DEMO {
        println!("  {imp:>6.1}  {title}");
    }
    let _ = std::fs::remove_file(&journal_path);
    Ok(())
}
