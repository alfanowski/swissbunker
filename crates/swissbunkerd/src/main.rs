//! `swissbunkerd` — the Forge, as a command line tool.
//!
//! The HTTP daemon and the wizard come later. This is the layer underneath both: it takes a
//! corpus that is already on the disk and turns it into an index the browser Reader can open
//! from `file://`, updating the disk's manifest as it goes.
//!
//! Usable on its own, deliberately. Someone filling a disk by hand should not have to wait for
//! a UI, and a CLI that works is also the thing the UI will end up calling.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use swissbunkerd::import::import;
use swissbunkerd::index::{build_index, Document};
use swissbunkerd::journal::{JobId, Journal};
use swissbunkerd::manifest::{CorpusEntry, Manifest};

const USAGE: &str = "\
swissbunkerd — build a SwissBunker disk

USAGE:
  swissbunkerd build <corpus> --disk <path> --id <id> [--name <name>] [--language <lang>]
  swissbunkerd status --disk <path>
  swissbunkerd serve --disk <path> [--port <n>] [--app <dir>]

ARGUMENTS:
  <corpus>            A JSONL file: one {\"title\":…,\"body\":…} object per line.
                      Optional per-line fields: \"source_path\", \"importance\".

OPTIONS:
  --disk <path>       The bunker disk, e.g. /Volumes/BUNKER
  --id <id>           Short identifier for this corpus, e.g. wikipedia_it
  --name <name>       Human name shown in the dashboard. Defaults to the id.
  --language <lang>   Two-letter code. Defaults to \"it\".
  --port <n>          Port for `serve`. Defaults to 7777.
  --app <dir>         Dashboard directory. Defaults to <disk>/app.

NOTES:
  Documents are indexed in the order they appear in the file, most important first.
  That order decides what a user sees for any search too broad to rank, so put the
  documents that matter at the top.
";

struct BuildArgs {
    corpus: PathBuf,
    disk: PathBuf,
    id: String,
    name: String,
    language: String,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("build") => cmd_build(parse_build(&args[1..])?),
        Some("status") => cmd_status(&flag(&args[1..], "--disk").context("--disk is required")?),
        Some("serve") => cmd_serve(&args[1..]),
        Some("-h") | Some("--help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => {
            eprintln!("unknown command: {other}\n");
            print!("{USAGE}");
            std::process::exit(2);
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<PathBuf> {
    flag_str(args, name).map(PathBuf::from)
}

fn flag_str(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn parse_build(args: &[String]) -> Result<BuildArgs> {
    let corpus = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .context("a corpus file is required")?;
    let disk = flag(args, "--disk").context("--disk is required")?;
    let id = flag_str(args, "--id").context("--id is required")?;
    let name = flag_str(args, "--name").unwrap_or_else(|| id.clone());
    let language = flag_str(args, "--language").unwrap_or_else(|| "it".to_string());
    Ok(BuildArgs {
        corpus,
        disk,
        id,
        name,
        language,
    })
}

/// Layout on the disk, per spec §5.
fn index_dir(disk: &Path) -> PathBuf {
    disk.join("index")
}
fn manifest_path(disk: &Path) -> PathBuf {
    disk.join("manifest.json")
}
fn state_dir(disk: &Path) -> PathBuf {
    disk.join(".state")
}

fn cmd_build(a: BuildArgs) -> Result<()> {
    if !a.corpus.exists() {
        bail!("no such file: {}", a.corpus.display());
    }
    if !a.disk.exists() {
        bail!("no such disk: {}", a.disk.display());
    }

    std::fs::create_dir_all(index_dir(&a.disk))?;
    std::fs::create_dir_all(state_dir(&a.disk))?;

    let journal = Journal::open(&state_dir(&a.disk).join("journal.db"))?;
    let job = JobId(a.id.clone());

    println!("reading {}", a.corpus.display());

    // The corpus is streamed into memory here, which is the honest limitation of this build:
    // the index builder wants an iterator it can restart, and a JSONL reader gives a one-shot
    // stream. Fine for corpora up to a few gigabytes of text; the daemon will need to stage
    // to disk before it can take a full Wikipedia. Stated rather than discovered.
    let mut docs: Vec<Document> = Vec::new();
    let stats = import(&a.corpus, &journal, &job, |d| {
        docs.push(d);
        Ok(())
    })?;

    if stats.skipped > 0 {
        println!("  {} lines skipped as unreadable", stats.skipped);
    }
    if stats.documents == 0 {
        bail!("no usable documents in {}", a.corpus.display());
    }
    println!(
        "  {} documents, ordering by {:?}",
        stats.documents, stats.signal
    );

    let out = index_dir(&a.disk).join(format!("{}.sqlite", a.id));
    println!("building {}", out.display());
    let built = build_index(docs.into_iter(), &out, &journal, &job)?;
    println!(
        "  {} documents, {:.1} MB, {:.1}s",
        built.documents,
        built.bytes as f64 / 1e6,
        built.build_secs
    );

    let mpath = manifest_path(&a.disk);
    let mut manifest = if mpath.exists() {
        Manifest::load(&mpath)?
    } else {
        Manifest::new()
    };
    manifest.add(CorpusEntry {
        id: a.id.clone(),
        name: a.name,
        language: a.language,
        documents: built.documents,
        index_bytes: built.bytes,
        index_file: format!("index/{}.sqlite", a.id),
        snapshot: String::new(),
        importance_signal: stats.signal,
        sha256: String::new(),
    });
    manifest.save(&mpath)?;

    println!(
        "\nbunker now holds {} documents across {} corpora",
        manifest.total_documents(),
        manifest.corpora.len()
    );
    println!("open START.html on the disk to search it");
    Ok(())
}

fn cmd_serve(args: &[String]) -> Result<()> {
    let disk = flag(args, "--disk").context("--disk is required")?;
    let port: u16 = flag_str(args, "--port")
        .unwrap_or_else(|| "7777".to_string())
        .parse()
        .context("--port must be a number")?;
    // Loopback is hard-coded rather than configurable: the address is the security boundary,
    // and a flag to widen it would be a flag to remove the boundary. See spec §10.
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

    let app = flag(args, "--app").or_else(|| Some(swissbunkerd::api::app_dir(&disk)));

    let runtime = tokio::runtime::Runtime::new().context("starting the async runtime")?;
    runtime.block_on(swissbunkerd::api::serve_with_app(disk, addr, app))
}

fn cmd_status(disk: &Path) -> Result<()> {
    let mpath = manifest_path(disk);
    if !mpath.exists() {
        println!("{} holds no bunker yet", disk.display());
        return Ok(());
    }
    let m = Manifest::load(&mpath)?;
    println!("{} — built by {}", disk.display(), m.built_by);
    println!(
        "{} corpora, {} documents, {:.1} MB of index\n",
        m.corpora.len(),
        m.total_documents(),
        m.total_index_bytes() as f64 / 1e6
    );
    for c in &m.corpora {
        println!(
            "  {:<20} {:>10} docs  {:>8.1} MB  ordered by {:?}",
            c.id,
            c.documents,
            c.index_bytes as f64 / 1e6,
            c.importance_signal
        );
    }
    Ok(())
}
