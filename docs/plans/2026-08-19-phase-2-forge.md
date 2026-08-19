# Fase 2 — Forge · Piano di implementazione

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un comando solo trasforma un disco vuoto in un bunker funzionante: `curl … | sh`
sceglie il disco, apre una dashboard, e da lì si sceglie cosa scaricare. Alla fine il disco
contiene un indice che il Reader della Fase 1 apre e interroga da `file://`.

**Architecture:** Un daemon Rust che vive **sul disco**, non sulla macchina. Espone un'API
locale e serve la stessa dashboard del Runtime. Quattro sottosistemi con un contratto
esplicito fra loro: `catalog` sa cosa esiste, `acquire` lo scarica in modo riprendibile,
`extract` ne ricava testo, `index` costruisce l'FTS5 — e il journal rende ogni passo
ripetibile dopo un crash o un cavo staccato.

**Tech Stack:** Rust (axum, tokio, rusqlite, reqwest), lo stesso TypeScript della Fase 1 per
la UI, SQLite FTS5 come formato di uscita.

**Spec:** [`docs/specs/2026-08-18-swissbunker-design.md`](../specs/2026-08-18-swissbunker-design.md) — §4.2 daemon, §5 layout, §6.5 ordine di inserimento, §9.2-9.3 wizard
**Evidenza di partenza:** [Fase 0](../reports/2026-08-19-phase-0-findings.md) · [Fase 1](../reports/2026-08-19-phase-1-verification.md)

---

## Nota sul livello di dettaglio

**I Task 1 e 2 contengono codice completo; dal Task 3 in poi no**, e la differenza è
deliberata, non una svista.

I primi due toccano solo SQLite e `serde`, API che conosco e che questo progetto ha già usato.
Dal terzo in poi si entra in `reqwest`, in una libreria ZIM e nell'API delle Server-Sent
Events: territorio dove questo progetto ha già buttato via codice **tre volte** per averlo
scritto contro firme immaginate — `FS.createLazyFile`, `VFS.Base`, e i due tentativi di VFS
della Fase 1.

Ogni task da 3 in poi comincia quindi con uno step di ispezione dell'API, e il codice si
scrive dopo. Un piano che riempisse quei task di codice plausibile sembrerebbe più completo
e sarebbe meno utile: darebbe un'illusione di certezza esattamente dove la certezza manca.

---

## Cambio di scope, 2026-08-19: la custodia, non il contenuto

**Il committente ha chiarito che la Forge non deve procurarsi i contenuti.** Li metterà sul
disco a mano. Questo sposta il baricentro della fase:

- **Il download manager (Task 3) scende di priorità.** Resta nel piano perché il prodotto
  finito lo vuole, ma non è più sul percorso critico e non blocca nulla.
- **Il cuore diventa l'import**: prendere un file che si trova già sul disco, ricavarne
  documenti e costruire l'indice. Il Task 4 va riscritto in questa chiave.
- **Nessun download di prova è necessario per sviluppare.** Le fixture si generano, e il
  contenuto vero arriva quando lo mette lui.

L'ordine di lavoro diventa: journal → indicizzazione → import → daemon → wizard → download.

---

## Percorso minimo, e cosa resta fuori

Questa fase costruisce un **walking skeleton**: il percorso completo dal comando al risultato,
con un solo corpus e senza fronzoli. Un piano che coprisse tutti e cinque i sottosistemi in
profondità non produrrebbe nulla di funzionante fino all'ultimo giorno, e ogni decisione presa
in mezzo sarebbe indietro rispetto a ciò che il percorso reale insegna.

**Dentro:** bootstrap, selezione del disco, daemon, un catalogo con pochi corpora, download
riprendibile e verificato, estrazione ZIM, indicizzazione FTS5 in ordine di importanza,
wizard essenziale, health check.

**Fuori, e dichiarato:**

| Escluso | Perché, e dove va |
|---|---|
| Embedding e indice vettoriale | Fase 3. Questa fase produce solo il ramo BM25 |
| Aggiornamenti differenziali | Fase 6. Serve prima un formato di indice stabile |
| Area personale cifrata | Fase 6 |
| Torrent | Fase 6. HTTP con mirror basta a provare il percorso |
| Tutti i preset di §9.3 | Uno solo, "Sopravvivenza ridotto", finché il percorso non regge |
| Firma dei binari | Fase 6. In sviluppo si esegue localmente |
| Windows e Linux | Il daemon si compila per tutti e tre, ma solo macOS è verificato qui |

---

## Global Constraints

Ereditati e misurati, non supposti:

- **Il daemon vive sul disco, mai installato sulla macchina.** Il bootstrap copia, non installa.
  Niente `sudo`, niente pacchetti di sistema, niente scritture fuori dal disco scelto.
- **exFAT.** Nessun permesso, nessun symlink, nessun journaling. Pochi file monolitici.
- **Indice costruito su disco interno, poi copiato.** Scrivere un B-tree SQLite direttamente
  su exFAT via USB è patologicamente lento per via del pattern di scrittura casuale.
- **Ordine di inserimento per importanza decrescente** (spec §6.5). Non è opzionale: metà
  delle ricerche reali finisce nel percorso non ordinato, dove l'ordine di inserimento *è*
  l'ordine dei risultati.
- **Ogni operazione lunga è riprendibile.** Un download di ore che riparte da zero dopo un
  cavo staccato non è un prodotto.
- **Il daemon ascolta solo su `127.0.0.1`.** Mai `0.0.0.0`.
- **Nessuna telemetria.** Nessuna connessione in uscita che non sia un download richiesto.
- Codice, identificatori e commenti in inglese. Documentazione di piano in italiano.

### Regole apprese, che valgono anche qui

> **Stampa l'API prima, scrivi codice dopo.** Questo progetto ha buttato via codice tre volte
> per averla indovinata: `FS.createLazyFile`, `VFS.Base`, e i due tentativi di VFS della
> Fase 1.
>
> **Misura la condizione reale, non quella comoda.** L'errore `locateFile` non esisteva su
> `http://` ed era fatale su `file://`. Ogni verifica di questa fase deve girare su exFAT,
> non su APFS.
>
> **Il tuo strumento ha una configurazione, e può essere la variabile che stai misurando.**
> Playwright disabilitava la policy che la Fase 0 misurava.

---

## Struttura dei file

| File | Responsabilità |
|---|---|
| `bootstrap/install.sh` | Il one-liner: rileva OS/arch, elenca i dischi, copia, avvia |
| `bootstrap/install.ps1` | Equivalente PowerShell per Windows |
| `crates/swissbunkerd/src/main.rs` | Avvio, configurazione, binding su loopback |
| `crates/swissbunkerd/src/api.rs` | API HTTP: stato, catalogo, avvio job, progresso |
| `crates/swissbunkerd/src/journal.rs` | Stato durevole di ogni operazione |
| `crates/swissbunkerd/src/catalog.rs` | Catalogo dei corpora, firmato e versionato |
| `crates/swissbunkerd/src/acquire.rs` | Download riprendibile, verificato, con mirror |
| `crates/swissbunkerd/src/extract.rs` | Da ZIM a documenti, con segnale di importanza |
| `crates/swissbunkerd/src/index.rs` | Costruzione FTS5 in ordine di importanza |
| `crates/swissbunkerd/src/manifest.rs` | Scrittura del `manifest.json` auto-descrittivo |
| `web/src/console/*` | Wizard: preset, catalogo, avanzamento, health check |
| `docs/reports/…-phase-2-verification.md` | Il deliverable finale |

**Perché il journal è un modulo e non un dettaglio di `acquire`.** Download, estrazione e
indicizzazione hanno tutti bisogno di riprendere, e ognuno riprende in modo diverso: un
download riparte da un offset di byte, un'estrazione da un indice di documento, un'indice da
un segmento. Un solo modulo che sa registrare "a che punto ero" per tutti e tre evita tre
implementazioni divergenti dello stesso problema — e la divergenza si scopre sempre dopo un
crash, cioè nel momento peggiore.

---

### Task 1: Journal — lo stato che sopravvive al cavo staccato · ✅ COMPLETATO 2026-08-19

> 13 test, verdi **sia su APFS sia su exFAT**. La verifica su exFAT prescritta dallo Step 6
> si è ripagata subito: 12 test su 13 fallivano lì con `SQLITE_READONLY_DBMOVED`. SQLite
> confronta l'inode per difendersi da un file sostituito, ed exFAT non ne ha di stabili.
> Risolto con `PRAGMA locking_mode = EXCLUSIVE`, ora vincolo V8 nella spec.

**Files:**
- Create: `crates/swissbunkerd/Cargo.toml`, `src/journal.rs`, `tests/journal.rs`

**Interfaces:**
- Consumes: niente
- Produces:
  ```rust
  pub enum Stage { Download, Extract, Index, Verify }
  pub struct JobId(pub String);

  pub struct Journal { /* … */ }
  impl Journal {
      pub fn open(path: &Path) -> Result<Self>;
      /// Record progress. Idempotent: the same position twice is not an error.
      pub fn mark(&self, job: &JobId, stage: Stage, position: u64, total: u64) -> Result<()>;
      /// Where to resume from, or None if this job never started.
      pub fn resume_point(&self, job: &JobId, stage: Stage) -> Result<Option<u64>>;
      pub fn complete(&self, job: &JobId, stage: Stage) -> Result<()>;
      pub fn is_complete(&self, job: &JobId, stage: Stage) -> Result<bool>;
      pub fn failed(&self, job: &JobId, stage: Stage, error: &str) -> Result<()>;
  }
  ```

**Il journal si scrive per primo** perché tutto il resto ne dipende, e perché è l'unico
componente il cui bug si manifesta solo quando qualcosa va storto — cioè quando non si ha
voglia di scoprirne uno.

- [ ] **Step 1: Scaffolding del crate**

```bash
cd ~/Desktop/SwissBunker
mkdir -p crates/swissbunkerd/src
cat > Cargo.toml <<'EOF'
[workspace]
members = ["crates/swissbunkerd"]
resolver = "2"
EOF
cat > crates/swissbunkerd/Cargo.toml <<'EOF'
[package]
name = "swissbunkerd"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1"
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dev-dependencies]
tempfile = "3"
EOF
cargo build 2>&1 | tail -3
```

- [ ] **Step 2: Scrivere i test che falliscono**

```rust
// crates/swissbunkerd/tests/journal.rs
use swissbunkerd::journal::{Journal, JobId, Stage};

fn temp_journal() -> (tempfile::TempDir, Journal) {
    let dir = tempfile::tempdir().unwrap();
    let j = Journal::open(&dir.path().join("journal.db")).unwrap();
    (dir, j)
}

#[test]
fn a_job_that_never_started_has_no_resume_point() {
    let (_d, j) = temp_journal();
    let job = JobId("wikipedia_it".into());
    assert_eq!(j.resume_point(&job, Stage::Download).unwrap(), None);
}

#[test]
fn progress_is_recoverable() {
    let (_d, j) = temp_journal();
    let job = JobId("wikipedia_it".into());
    j.mark(&job, Stage::Download, 1_000_000, 40_000_000).unwrap();
    assert_eq!(j.resume_point(&job, Stage::Download).unwrap(), Some(1_000_000));
}

#[test]
fn progress_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.db");
    let job = JobId("wikipedia_it".into());
    {
        let j = Journal::open(&path).unwrap();
        j.mark(&job, Stage::Download, 5_000, 10_000).unwrap();
    } // dropped, as if the process were killed
    let j = Journal::open(&path).unwrap();
    assert_eq!(j.resume_point(&job, Stage::Download).unwrap(), Some(5_000));
}

#[test]
fn marking_the_same_position_twice_is_not_an_error() {
    let (_d, j) = temp_journal();
    let job = JobId("x".into());
    j.mark(&job, Stage::Download, 42, 100).unwrap();
    j.mark(&job, Stage::Download, 42, 100).unwrap();
    assert_eq!(j.resume_point(&job, Stage::Download).unwrap(), Some(42));
}

#[test]
fn stages_are_independent() {
    let (_d, j) = temp_journal();
    let job = JobId("x".into());
    j.mark(&job, Stage::Download, 100, 100).unwrap();
    j.complete(&job, Stage::Download).unwrap();
    assert!(j.is_complete(&job, Stage::Download).unwrap());
    assert!(!j.is_complete(&job, Stage::Index).unwrap());
    assert_eq!(j.resume_point(&job, Stage::Index).unwrap(), None);
}

#[test]
fn a_completed_stage_reports_complete_after_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.db");
    let job = JobId("x".into());
    {
        let j = Journal::open(&path).unwrap();
        j.complete(&job, Stage::Extract).unwrap();
    }
    let j = Journal::open(&path).unwrap();
    assert!(j.is_complete(&job, Stage::Extract).unwrap());
}

#[test]
fn a_failure_is_recorded_without_losing_the_resume_point() {
    // A failed download must still resume from where it got to: throwing away the position
    // on error would turn every transient network fault into a restart from zero.
    let (_d, j) = temp_journal();
    let job = JobId("x".into());
    j.mark(&job, Stage::Download, 900, 1000).unwrap();
    j.failed(&job, Stage::Download, "connection reset").unwrap();
    assert_eq!(j.resume_point(&job, Stage::Download).unwrap(), Some(900));
    assert!(!j.is_complete(&job, Stage::Download).unwrap());
}
```

- [ ] **Step 3: Verificare che falliscano**

```bash
cd ~/Desktop/SwissBunker && cargo test -p swissbunkerd --test journal 2>&1 | tail -5
```

Atteso: errore di compilazione, il modulo non esiste.

- [ ] **Step 4: Implementare**

```rust
// crates/swissbunkerd/src/journal.rs
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

/// The stages every corpus passes through. Kept as an enum rather than a string so a typo
/// becomes a compile error instead of a job that silently never resumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage { Download, Extract, Index, Verify }

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Stage::Download => "download",
            Stage::Extract => "extract",
            Stage::Index => "index",
            Stage::Verify => "verify",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobId(pub String);

/// Durable record of how far each job got.
///
/// SQLite rather than a JSON file that gets rewritten: a rewrite that is interrupted halfway
/// leaves a truncated file, and the one moment this data matters is exactly the moment
/// something was interrupted halfway.
pub struct Journal {
    conn: Mutex<Connection>,
}

impl Journal {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening journal at {}", path.display()))?;
        // WAL is unavailable on exFAT in practice, and this table sees a write every few
        // seconds at most, so the default rollback journal is both sufficient and safer here.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS progress (
                 job       TEXT NOT NULL,
                 stage     TEXT NOT NULL,
                 position  INTEGER NOT NULL DEFAULT 0,
                 total     INTEGER NOT NULL DEFAULT 0,
                 done      INTEGER NOT NULL DEFAULT 0,
                 error     TEXT,
                 PRIMARY KEY (job, stage)
             );",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn mark(&self, job: &JobId, stage: Stage, position: u64, total: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Recording progress clears any previous error: the job is evidently alive again.
        conn.execute(
            "INSERT INTO progress (job, stage, position, total, done, error)
                  VALUES (?1, ?2, ?3, ?4, 0, NULL)
             ON CONFLICT(job, stage) DO UPDATE
                  SET position = excluded.position,
                      total    = excluded.total,
                      error    = NULL",
            params![job.0, stage.as_str(), position as i64, total as i64],
        )?;
        Ok(())
    }

    pub fn resume_point(&self, job: &JobId, stage: Stage) -> Result<Option<u64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT position FROM progress WHERE job = ?1 AND stage = ?2")?;
        let mut rows = stmt.query(params![job.0, stage.as_str()])?;
        Ok(match rows.next()? {
            Some(r) => Some(r.get::<_, i64>(0)? as u64),
            None => None,
        })
    }

    pub fn complete(&self, job: &JobId, stage: Stage) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO progress (job, stage, position, total, done, error)
                  VALUES (?1, ?2, 0, 0, 1, NULL)
             ON CONFLICT(job, stage) DO UPDATE SET done = 1, error = NULL",
            params![job.0, stage.as_str()],
        )?;
        Ok(())
    }

    pub fn is_complete(&self, job: &JobId, stage: Stage) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT done FROM progress WHERE job = ?1 AND stage = ?2")?;
        let mut rows = stmt.query(params![job.0, stage.as_str()])?;
        Ok(match rows.next()? {
            Some(r) => r.get::<_, i64>(0)? != 0,
            None => false,
        })
    }

    /// Record a failure WITHOUT discarding the resume point: throwing away the position on
    /// error would turn every transient network fault into a restart from zero.
    pub fn failed(&self, job: &JobId, stage: Stage, error: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO progress (job, stage, position, total, done, error)
                  VALUES (?1, ?2, 0, 0, 0, ?3)
             ON CONFLICT(job, stage) DO UPDATE SET error = excluded.error, done = 0",
            params![job.0, stage.as_str(), error],
        )?;
        Ok(())
    }
}
```

- [ ] **Step 5: Verificare che passino**

```bash
cd ~/Desktop/SwissBunker && cargo test -p swissbunkerd --test journal 2>&1 | tail -5
```

Atteso: 7 test PASS.

- [ ] **Step 6: Provarlo su exFAT, non solo su APFS**

I test girano in `tempdir()`, cioè su APFS. Il bunker vive su exFAT, dove il locking di
SQLite si comporta diversamente:

```bash
hdiutil attach ~/swissbunker-fixtures/exfat-test.sparseimage
TMPDIR=/Volumes/SWISSTEST cargo test -p swissbunkerd --test journal 2>&1 | tail -5
```

Se qui fallisce qualcosa che su APFS passava, **è quello il comportamento vero** e il journal
va adattato, non il test.

- [ ] **Step 7: Commit**

```bash
cd ~/Desktop/SwissBunker
git add Cargo.toml crates
git commit -m "feat(forge): durable job journal, verified on exFAT"
```

---

### Task 2: Catalogo

**Files:**
- Create: `crates/swissbunkerd/src/catalog.rs`, `tests/catalog.rs`
- Create: `catalog/catalog.json`

**Interfaces:**
- Consumes: niente
- Produces:
  ```rust
  pub struct Corpus {
      pub id: String,           // "wikipedia_it"
      pub name: String,         // "Wikipedia italiana"
      pub description: String,  // human sentence, not a filename
      pub language: String,
      pub bytes: u64,
      pub sha256: String,
      pub mirrors: Vec<String>, // tried in order
      pub snapshot: String,     // "2026-06"
      pub documents: u64,
      pub samples: Vec<String>, // three real titles from inside
  }
  pub struct Catalog { /* … */ }
  impl Catalog {
      pub fn load(path: &Path) -> Result<Self>;
      pub fn get(&self, id: &str) -> Option<&Corpus>;
      pub fn all(&self) -> &[Corpus];
      /// Total bytes on disk after building, not just the download.
      pub fn projected_size(&self, ids: &[&str]) -> u64;
  }
  ```

**`projected_size` esiste perché il wizard ha sbagliato una volta già sulla carta.** La
revisione della spec ha trovato che i preset dichiaravano i byte scaricati mentre la barra
mostrava il totale a disco, e i due differiscono di ~90 GB. Il calcolo sta nel catalogo, in un
posto solo, testato.

- [ ] **Step 1: Scrivere il catalogo iniziale**

Tre corpora soltanto, per far camminare lo scheletro. `samples` contiene titoli veri presi
dal contenuto, perché la spec §9.3 chiede che si sappia cosa si sta scaricando prima di
scaricarlo.

```json
{
  "version": 1,
  "generated": "2026-08-19",
  "corpora": [
    {
      "id": "wikipedia_it_nopic",
      "name": "Wikipedia italiana",
      "description": "L'enciclopedia in italiano, senza immagini",
      "language": "it",
      "bytes": 15800000000,
      "sha256": "",
      "mirrors": [
        "https://download.kiwix.org/zim/wikipedia/wikipedia_it_all_nopic.zim"
      ],
      "snapshot": "2026-06",
      "documents": 1900000,
      "samples": ["Fotosintesi clorofilliana", "Battaglia di Lepanto", "Acqua potabile"],
      "indexRatio": 0.42
    }
  ]
}
```

> **`sha256` vuoto significa "non ancora verificabile", e il Task 3 deve rifiutarsi di
> considerare completo un download il cui corpus non ha hash** — piuttosto che accettarlo in
> silenzio. Si riempie scaricando il file una volta e registrandone l'hash; finché è vuoto,
> quel corpus è marcato come non verificato nel manifest.
>
> **`indexRatio` è una stima e va marcata come tale.** È la frazione della dimensione del
> corpus che l'indice occuperà, e finché la Fase 2 non ne avrà costruito uno vero è un numero
> preso dalla spec §11.1, non una misura. Il Task 8 lo sostituisce con quello osservato.

- [ ] **Step 2: Test che falliscono**

```rust
// crates/swissbunkerd/tests/catalog.rs
use swissbunkerd::catalog::Catalog;
use std::path::Path;

fn catalog() -> Catalog {
    Catalog::load(Path::new("../../catalog/catalog.json")).unwrap()
}

#[test]
fn loads_the_shipped_catalog() {
    assert!(!catalog().all().is_empty());
}

#[test]
fn finds_a_corpus_by_id() {
    let c = catalog();
    let w = c.get("wikipedia_it_nopic").expect("wikipedia_it_nopic missing");
    assert_eq!(w.language, "it");
    assert!(w.bytes > 0);
}

#[test]
fn every_corpus_has_human_readable_metadata() {
    // Spec §9.3: the wizard shows names and examples, never filenames. A corpus without
    // them would force the UI to fall back to the id, which is exactly what Nomad does.
    for c in catalog().all() {
        assert!(!c.name.is_empty(), "{} has no name", c.id);
        assert!(!c.description.is_empty(), "{} has no description", c.id);
        assert!(c.samples.len() >= 3, "{} has fewer than 3 samples", c.id);
        assert!(!c.mirrors.is_empty(), "{} has no mirrors", c.id);
    }
}

#[test]
fn projected_size_includes_the_index_not_just_the_download() {
    let c = catalog();
    let w = c.get("wikipedia_it_nopic").unwrap();
    let projected = c.projected_size(&["wikipedia_it_nopic"]);
    // The wizard's progress bar shows total-on-disk. Reporting only the download would
    // understate it by roughly the index ratio, which is how a disk fills up unexpectedly.
    assert!(projected > w.bytes, "projected {} not greater than download {}", projected, w.bytes);
}

#[test]
fn an_unknown_id_is_none_rather_than_a_panic() {
    assert!(catalog().get("does_not_exist").is_none());
}
```

- [ ] **Step 3: Implementare, verificare, committare**

```bash
cd ~/Desktop/SwissBunker && cargo test -p swissbunkerd --test catalog 2>&1 | tail -5
git add crates catalog && git commit -m "feat(forge): corpus catalog with human metadata and projected size"
```

---

### Task 3: Download riprendibile

**Files:**
- Create: `crates/swissbunkerd/src/acquire.rs`, `tests/acquire.rs`

**Interfaces:**
- Consumes: `Journal` (Task 1), `Catalog` (Task 2)
- Produces:
  ```rust
  pub struct Progress { pub done: u64, pub total: u64, pub bytes_per_sec: f64 }

  pub async fn download(
      corpus: &Corpus,
      dest: &Path,
      journal: &Journal,
      on_progress: impl Fn(Progress),
  ) -> Result<()>;
  ```

**Tre proprietà, ognuna con il suo test.** Riprende da dove era; verifica l'hash e rifiuta un
file corrotto; passa al mirror successivo quando uno cade. Nessuna delle tre è opzionale su
un download da ore.

- [ ] **Step 1: Test contro un server locale che si comporta male apposta**

```rust
// crates/swissbunkerd/tests/acquire.rs
//
// The tests run against a local server that misbehaves deliberately: truncating responses,
// refusing ranges, serving wrong bytes. A download manager that has only ever been tested
// against a well-behaved server has not been tested.

#[tokio::test]
async fn resumes_from_the_recorded_offset() {
    // Serve the first half, kill the connection, then check the second attempt asks for
    // Range: bytes=<half>- rather than starting over.
    // …
}

#[tokio::test]
async fn rejects_a_file_whose_hash_does_not_match() {
    // A corrupt corpus that silently passes is worse than a failed download: it fails later,
    // during indexing, with a confusing error and hours already spent.
    // …
}

#[tokio::test]
async fn falls_through_to_the_next_mirror() {
    // …
}

#[tokio::test]
async fn a_server_that_ignores_range_requests_restarts_cleanly() {
    // Some mirrors answer 200 with the whole file when sent a Range header. Appending that
    // to a partial file would corrupt it in a way the hash catches only at the very end.
    // …
}
```

> **I corpi dei test sono da scrivere insieme all'implementazione**, non prima: il contratto
> con il server finto dipende da quale client HTTP viene scelto, e questo piano non lo fissa
> per non ripetere l'errore di scrivere codice contro un'API non ispezionata.

- [ ] **Step 2: Ispezionare l'API prima di scriverla**

```bash
cd ~/Desktop/SwissBunker
cargo add reqwest --features stream,rustls-tls --dry-run
cargo doc -p reqwest --no-deps --open   # confirm the Range and stream API shape
```

- [ ] **Step 3: Implementare, verificare, committare**

---

### Nota emersa dal primo end-to-end: i file AppleDouble

Scrivendo su exFAT da macOS il disco si riempie di file `._nome` accanto a ogni file reale —
sono i resource fork che macOS emula sui filesystem che non li supportano. Non rompono nulla,
ma **su Windows compaiono come spazzatura** accanto a ogni contenuto, il che su un prodotto
che si vanta di essere pulito non è accettabile.

Da affrontare prima del rilascio: `COPYFILE_DISABLE=1` durante la scrittura, `dot_clean` alla
fine di una build, oppure entrambi. Registrato ora perché si nota solo guardando il disco, e
si smette di notarlo dopo la decima volta.

### Task 4: Import da file locale · ✅ COMPLETATO 2026-08-19 (JSONL)

> Riscritto secondo il cambio di scope: importa da un file già presente sul disco invece di
> estrarre da uno ZIM scaricato. 13 test.
>
> **JSONL come formato nativo**: una riga per documento, streaming senza caricare il corpus in
> memoria, sopravvive a un'ultima riga troncata, e chiunque può produrlo con uno script. Il
> formato si riconosce dai magic byte prima che dall'estensione, perché un file rinominato a
> mano è normale su un disco riempito a mano.
>
> **Lo ZIM è riconosciuto e rifiutato con istruzioni**, non ignorato: leggerlo richiede un
> binding libzim, e la regola del progetto è ispezionare un'API prima di scriverci contro.
>
> Il segnale di importanza di default è l'ordine della sorgente. Se il JSONL porta valori
> espliciti, il manifest registra `Explicit` e **non** inventa una provenienza che non conosce.

### Task 4 (originale): Estrazione da ZIM — rinviata

**Files:**
- Create: `crates/swissbunkerd/src/extract.rs`, `tests/extract.rs`

**Interfaces:**
- Consumes: `Journal`
- Produces:
  ```rust
  pub struct Document {
      pub title: String,
      pub body: String,
      pub source_path: String,   // path inside the ZIM, for later retrieval
      pub importance: f64,       // higher is more important
  }
  pub fn extract_zim(
      path: &Path, journal: &Journal, job: &JobId,
      on_document: impl FnMut(Document) -> Result<()>,
  ) -> Result<u64>;
  ```

**`importance` è il campo che giustifica questa fase.** La Fase 1 ha misurato che metà delle
ricerche reali su una Wikipedia completa finisce nel percorso non ordinato, dove i risultati
escono in ordine di inserimento. Quindi questo numero decide cosa vede l'utente in metà dei
casi (spec §6.5).

**Come calcolarlo, in ordine di preferenza:**

1. **Link entranti** — il proxy classico di rilevanza enciclopedica, ricavabile contando i
   riferimenti interni durante l'estrazione. Costa una passata in più.
2. **Pageviews aggregate** — accurate, ma richiedono una fonte esterna e quindi rete durante
   la build.
3. **Lunghezza dell'articolo** — **provata in Fase 1 e non funziona**: ordinando per lunghezza
   i risultati non sono migliori di quelli arbitrari. Ammessa solo come ultima spiaggia,
   dichiarandolo nel manifest.

> **Decisione da prendere all'inizio del task**, misurando il costo della passata in più
> sui link entranti contro il beneficio. La Fase 1 ha dimostrato il meccanismo con un giorno
> di pageviews e ha visto i risultati piegarsi verso lo sport del momento: un segnale
> instabile è peggio di uno grezzo ma stabile.

- [ ] **Step 1: Ispezionare il formato ZIM e la libreria scelta**
- [ ] **Step 2: Test su uno ZIM piccolo reale, non su un mock**
- [ ] **Step 3: Misurare il costo del calcolo dei link entranti**
- [ ] **Step 4: Implementare, verificare, committare**

---

### Task 5: Indicizzazione in ordine di importanza · ✅ COMPLETATO 2026-08-19

> 9 test verdi su APFS ed exFAT, più **6 check di conformità cross-language**: un indice
> costruito dal Forge in Rust, aperto dal Reader nel browser da `file://`, con l'ordine di
> importanza intatto attraverso i due linguaggi. Criteri di uscita 4 e 5 soddisfatti.
>
> Costruzione a due passate: staging con l'importanza, poi rilettura `ORDER BY importance
> DESC` dentro FTS5. Costa disco temporaneo pari al testo del corpus e compra la proprietà
> che rende utile la ricerca non ordinata.

**Files:**
- Create: `crates/swissbunkerd/src/index.rs`, `tests/index.rs`

**Interfaces:**
- Consumes: `Document` (Task 4), `Journal`
- Produces:
  ```rust
  pub struct IndexStats { pub documents: u64, pub bytes: u64, pub build_secs: f64 }
  pub fn build_index(
      docs: impl Iterator<Item = Document>,
      out: &Path, journal: &Journal, job: &JobId,
  ) -> Result<IndexStats>;
  ```

**Il requisito che questo task esiste per soddisfare** è spec §6.5: i documenti entrano in
ordine di importanza decrescente. È una `ORDER BY importance DESC` sull'iteratore di input,
una riga — ma senza di essa metà delle ricerche restituisce risultati arbitrari.

- [ ] **Step 1: Test che l'ordine di inserimento sia rispettato**

```rust
#[test]
fn documents_are_inserted_most_important_first() {
    // Build an index from documents with known importance, then read back the rowids: FTS5
    // returns unranked matches in rowid order, so rowid order IS result order for half of
    // all real searches (spec §6.5, measured in the phase 1 report §4bis).
    // …
}

#[test]
fn the_index_is_readable_by_the_phase_1_reader() {
    // The whole point: an index this Forge builds must open in the browser reader. Anything
    // else is two products that happen to share a file extension.
    // …
}
```

- [ ] **Step 2: Costruire su disco interno, copiare su exFAT**

Misurare entrambe le strade e registrare il rapporto: la spec §6.4 afferma che scrivere
direttamente su exFAT via USB è patologicamente lento, ed è un'affermazione mai misurata.

- [ ] **Step 3: Implementare, verificare, committare**

---

### Task 6: Daemon e API

**Files:**
- Create: `crates/swissbunkerd/src/main.rs`, `src/api.rs`, `tests/api.rs`

**Interfaces:**
- Produces l'API che la Console consuma:
  ```
  GET  /api/state          → { disk, corpora: [...], jobs: [...] }
  GET  /api/catalog        → the catalog, with projected sizes
  POST /api/build          → { corpora: ["wikipedia_it_nopic"] } starts the pipeline
  GET  /api/progress       → server-sent events: stage, position, total, eta
  POST /api/pause          → suspend, leaving the journal intact
  GET  /api/health         → runs the health check of spec §9.3 screen 5
  ```

- [ ] **Step 1: Binding solo su loopback, con un test che lo prova**

```rust
#[tokio::test]
async fn the_daemon_refuses_to_listen_on_a_public_interface() {
    // Spec §10. A bunker daemon reachable from the network is a file server nobody asked
    // for, on a machine that is not the owner's.
    // …
}
```

- [ ] **Step 2-4: API, eventi di progresso, health check**

---

### Task 7: Bootstrap e wizard

**Files:**
- Create: `bootstrap/install.sh`
- Create: `web/src/console/*`

- [ ] **Step 1: Lo script del one-liner**

Deve fare, in quest'ordine: rilevare OS e architettura, elencare i dischi con spazio e
velocità misurata, far scegliere, verificare il filesystem, copiare daemon e dashboard,
avviare, aprire il browser. **Non formatta mai nulla** senza conferma esplicita e ripetuta.

Il benchmark del disco (§9.2) è ~5 secondi di lettura e scrittura, e serve a fermare qualcuno
prima che spenda sei ore di download su un supporto inadeguato.

- [ ] **Step 2: Il wizard, ridotto a un preset**

Schermi 1, 3, 4 e 5 di spec §9.3. Lo schermo 2 (catalogo completo) arriva quando i corpora
sono più di tre.

- [ ] **Step 3: Health check che apre il Reader vero**

Non una simulazione: carica il Reader della Fase 1 sull'indice appena costruito e ci esegue
cinque ricerche reali, mostrando i tempi.

---

### Task 8: Verifica end-to-end

- [ ] **Step 1: Da disco vuoto a ricerca funzionante, senza interventi manuali**

```bash
# On a freshly formatted exFAT volume:
curl -fsSL file://$PWD/bootstrap/install.sh | sh
# choose the disk, choose the preset, wait
# then: open START.html from the disk and search
```

- [ ] **Step 2: Interrompere e riprendere, tre volte**

Staccare il cavo durante il download, durante l'estrazione e durante l'indicizzazione.
Ognuna delle tre deve riprendere senza rifare il lavoro già fatto. **È il test che giustifica
il journal**, ed è l'unico modo di sapere se funziona.

- [ ] **Step 3: Sostituire `indexRatio` stimato con quello misurato**

- [ ] **Step 4: Scrivere il report di verifica**

Stessa disciplina delle fasi precedenti: numeri misurati, criteri soddisfatti o no, e una
sezione esplicita su ciò che **non** è stato dimostrato.

---

## Criteri di uscita della Fase 2

1. `curl … | sh` su un disco vuoto porta a un bunker interrogabile, senza passaggi manuali.
2. Download, estrazione e indicizzazione riprendono tutti dopo un'interruzione, verificato
   staccando davvero il cavo.
3. Un corpus corrotto viene rifiutato per hash, non scoperto durante l'indicizzazione.
4. L'indice prodotto si apre nel Reader della Fase 1 da `file://` e risponde alle ricerche.
5. I documenti sono inseriti in ordine di importanza decrescente, verificato leggendo i rowid.
6. Il daemon ascolta solo su loopback, verificato da un test.
7. Il manifest descrive il disco abbastanza da permettere alla dashboard di aprirsi senza
   scansionare i contenuti.
8. Il report di verifica esiste e dichiara i propri limiti.

## Cosa NON fa questa fase

- Nessun embedding, nessun indice vettoriale, nessuna ricerca ibrida. Fase 3.
- Nessun LLM. Fase 4.
- Nessun design definitivo: il wizard è funzionale, la Console è Fase 5.
- Nessun aggiornamento differenziale, nessuna cifratura, nessuna firma dei binari. Fase 6.
- Nessuna verifica su Windows o Linux, benché il daemon vi si compili.
