# SwissBunker — Design Document

**Versione:** 1.0
**Data:** 2026-08-18
**Autore:** Andrea "Alfanowski" Alfano
**Stato:** Fase 0 completata — GO CON MODIFICHE
**Evidenza:** [findings Fase 0](../reports/2026-08-19-phase-0-findings.md) · 48 record, 6 probe, 4 motori

---

## 1. Executive summary

SwissBunker è un **bunker digitale portatile**: un disco esterno che contiene una copia
offline, indicizzata e interrogabile del sapere umano di pubblico dominio — Wikipedia,
libri, sapere pratico, mappe — consultabile da **qualunque PC, senza installare nulla**,
tramite una dashboard web che gira nel browser e sfrutta la GPU della macchina ospite per
l'inferenza di un LLM locale.

Il sistema è **un solo prodotto con due modalità operative**, decise a runtime:

- **Connected** — un daemon (residente sul disco, non sul PC) serve la dashboard su
  `localhost`. Sblocca acquisizione contenuti, indicizzazione, aggiornamenti.
- **Portable** — nessun processo nativo. Si apre `START.html` dal disco e tutto gira
  client-side: ricerca, RAG, inferenza LLM via WebGPU.

Lo stesso codice, la stessa UI, la stessa estetica. In Portable le funzioni di gestione non
compaiono.

### Cosa NON è

- Non è un homelab server. Non richiede Proxmox, Docker, o una macchina dedicata.
- Non è un mirror di contenuti protetti da copyright. Solo materiale open o pubblico dominio.
- Non è un sostituto di internet. È ciò che resta quando internet non c'è.

---

## 2. Critica di riferimento: Project Nomad

Project Nomad (crosstalk-solutions) risolve un problema simile, ma per un pubblico diverso:
l'homelabber con un server fisso. Non è "fatto male" — è di un'altra categoria. I punti dove
SwissBunker diverge deliberatamente:

| Nomad | SwissBunker |
|---|---|
| Stack `docker-compose` su Proxmox | Zero dipendenze runtime sul PC ospite |
| Richiede admin, Docker, rete configurata | Nessun permesso richiesto in modalità Portable |
| N servizi, N web UI con look diversi | Una dashboard, un design system |
| Catalogo di file `.zim` con nomi tecnici | Preset curati + catalogo con nomi umani e preview |
| Download non riprendibili, nessuna verifica | Download resumable, hash-verified, con mirror fallback |
| Nessun motore di ricerca trasversale | Indice unico ibrido su tutti i corpora |
| Nessun LLM integrato al retrieval | RAG nativo con citazioni verificabili |
| Legato alla macchina su cui è installato | Legato al disco, non alla macchina |

**Il principio guida:** ogni decisione che aumenta il numero di cose che l'utente deve
installare, configurare o capire va rifiutata, anche a costo di prestazioni.

---

## 3. Requisiti

### 3.1 Funzionali

- **F1** — Bootstrap via one-liner (`curl … | sh` su Unix, `irm … | iex` su Windows) che
  seleziona il disco di destinazione e vi installa dashboard + daemon.
- **F2** — Dashboard con wizard di selezione contenuti: preset, catalogo, budget di spazio
  in tempo reale, stima tempi.
- **F3** — Acquisizione contenuti riprendibile, verificata per hash, con mirror fallback.
- **F4** — Costruzione incrementale degli indici (full-text e vettoriale) con journal
  riprendibile.
- **F5** — Ricerca ibrida (BM25 + densa + reranking) su tutti i corpora, con filtri per
  fonte, lingua e tipo.
- **F6** — Lettura dei contenuti originali (articoli, libri, mappe) dentro la dashboard.
- **F7** — Chat RAG con LLM locale, risposte **sempre** corredate di citazioni cliccabili
  che aprono il passaggio esatto nella fonte.
- **F8** — Import di documenti personali dal Runtime (drag&drop PDF/EPUB/MD), con
  chunking ed embedding lato browser.
- **F9** — Area personale cifrata lato client (WebCrypto, chiave derivata da passphrase).
- **F10** — Aggiornamento differenziale dei corpora.
- **F11** — Health check post-build che verifica il bunker con query reali.

### 3.2 Non funzionali

| ID | Requisito | Target |
|---|---|---|
| NF1 | Time-to-first-answer, disco freddo, PC mai visto | < 90 s |
| NF2 | Latenza ricerca ibrida (p95) | < 800 ms |
| NF3 | Throughput LLM su hardware Tier 2 | > 10 tok/s |
| NF4 | Recall@10 su golden set di 200 domande | > 0.85 |
| NF5 | Installazioni richieste sul PC ospite | 0 |
| NF6 | Privilegi amministrativi richiesti (Portable) | 0 |
| NF7 | Browser supportati (Portable) | Chrome/Edge 113+, Safari 26+, Firefox 147+ |
| NF8 | Funzionamento senza rete | Totale |

### 3.3 Vincoli tecnici duri

- **V1 — Filesystem exFAT.** Unico FS scrivibile nativamente su Windows/macOS/Linux senza
  driver. Conseguenze: nessun permesso, nessun symlink, nessun journaling, pessime
  prestazioni con molti file piccoli. **Il layout deve usare pochi file monolitici.**
- **V2 — Origin nullo su `file://`.** Niente `SharedArrayBuffer` (quindi niente WASM
  multi-thread su CPU), moduli ES bloccati, `IndexedDB` inaffidabile, `fetch` verso file
  locali bloccato. Tutto aggirabile, ma da verificare empiricamente (vedi Fase 0).
- **V3 — File System Access API solo Chromium.** Mitigato: il bunker è read-only, quindi si
  usa `<input type="file" webkitdirectory>` (supporto universale) + `File.slice()` per il
  random access. La scrittura avviene solo in OPFS (sandbox) o via daemon.
- **V4 — Limiti di memoria browser.** Heap WASM32 = 4 GB (**misurato**: limite heap JS
  3760 MB su Apple M4). `maxBufferSize` WebGPU **varia per GPU e va rilevato a runtime, mai
  assunto**: la stima iniziale di ~2 GB è stata smentita da una misura di **4.29 GB** su
  Apple M4. Il tier di modello si decide dal valore letto, non da una tabella statica.
- **V7 — OPFS non disponibile da `file://`.** Misurato: `opfs_usable` fallisce su Chromium,
  Chrome e WebKit; passa solo su Firefox. Lo storage utente in modalità Portable usa
  **IndexedDB**, che passa su tutti e quattro i motori.
- **V5 — Budget di calcolo per la build.** Macchina di riferimento: **Apple M4 base, 16 GB
  RAM**. Nessun cluster, nessuna GPU discreta.
- **V6 — Legalità.** Solo contenuti pubblico dominio o con licenza libera.

### 3.4 Non-obiettivi (YAGNI)

- Sincronizzazione multi-disco o multi-utente.
- Editing collaborativo.
- Fine-tuning dei modelli.
- Qualsiasi funzionalità che richieda un server remoto in fase d'uso.
- Supporto a browser precedenti al rilascio Baseline di WebGPU (gennaio 2026).

---

## 4. Architettura

### 4.1 Vista d'insieme

```
                    ┌──────────────── IL DISCO ────────────────┐
                    │                                          │
   curl | sh  ────► │  bin/     daemon nativi (per OS)          │
   (bootstrap)      │  app/     dashboard (JS + WASM)           │
                    │  content/ corpora monolitici              │
                    │  index/   indici FTS + vettoriali         │
                    │  models/  pesi ONNX/GGUF/MLC              │
                    │  personal/ area cifrata utente            │
                    └──────────────────────────────────────────┘
                              │                      │
              MODALITÀ CONNECTED          MODALITÀ PORTABLE
                              │                      │
                    ┌─────────▼────────┐   ┌─────────▼─────────┐
                    │ daemon su :7777  │   │  START.html       │
                    │ (dal disco)      │   │  (file://)        │
                    │ • download       │   │  • sola lettura   │
                    │ • indicizzazione │   │  • RAG + LLM      │
                    │ • embedding      │   │  • import locale  │
                    └─────────┬────────┘   └─────────┬─────────┘
                              │                      │
                              └──────► BROWSER ◄─────┘
                                  (stessa dashboard,
                                   capability detection)
```

### 4.2 Componenti

Ogni componente ha una responsabilità sola e un'interfaccia esplicita.

#### `bootstrap` — lo script del one-liner
**Cosa fa:** rileva OS/arch, scarica il daemon corretto, elenca i dischi disponibili con
spazio e velocità, fa scegliere la destinazione, copia daemon + dashboard sul disco, avvia
il daemon, apre il browser.
**Dipende da:** solo `curl`/`PowerShell` e una shell POSIX.
**Non fa:** nessuna scrittura fuori dal disco scelto. Nessun `sudo`. Nessun pacchetto di sistema.

Include un **benchmark del disco** (~5 s di lettura/scrittura sequenziale e random) che
classifica il supporto — `NVMe/USB4`, `SSD/USB3`, `HDD o USB2 (sconsigliato)` — e avvisa
prima che l'utente sprechi sei ore di download su un disco inadeguato.

#### `swissbunkerd` — il daemon (Rust)
**Cosa fa:** espone un'API HTTP locale su `127.0.0.1:7777` e serve la dashboard.
Sottosistemi:
- `catalog` — catalogo dei corpora disponibili (JSON firmato, con mirror e hash)
- `acquire` — download manager: parallelo, riprendibile, verificato, torrent→HTTP fallback
- `extract` — estrazione testo dai formati sorgente verso lo store canonico
- `chunker` — segmentazione in unità recuperabili
- `embedder` — inferenza dell'embedder via ONNX Runtime con CoreML/CUDA/DirectML
- `indexer` — costruzione FTS5 e IVF, incrementale e riprendibile
- `journal` — stato durevole di ogni operazione, per riprendere dopo un crash

**Non fa:** nessuna logica di presentazione. La dashboard è un client dell'API, non una parte
del daemon.

#### `console` — la dashboard (TypeScript, no framework pesanti)
**Cosa fa:** l'unica interfaccia utente del sistema. Rileva la modalità disponibile e adatta
le funzioni esposte.
**Dipende da:** l'API del daemon (Connected) oppure il `reader` (Portable).

#### `reader` — il motore di lettura client-side (TypeScript + WASM)
**Cosa fa:** in modalità Portable è ciò che sostituisce il daemon. Legge gli indici
direttamente dai `File` selezionati:
- VFS SQLite su `File.slice()` per interrogare `fts.sqlite` e `docstore.sqlite` a pagine
- lettore IVF per la ricerca vettoriale
- lettore ZIM per i contenuti originali
- fusione RRF e reranking

**Il punto chiave:** il `reader` è usato **anche** in modalità Connected. Non esistono due
implementazioni della ricerca. Il daemon costruisce gli indici; il `reader` è l'unico a
leggerli, sempre, in entrambe le modalità. Questo elimina alla radice la classe di bug in cui
la ricerca "funziona a casa ma non sul disco".

#### `inference` — il runtime LLM in-browser
**Cosa fa:** rileva l'hardware, sceglie il tier di modello, carica i pesi dal `FileList`,
esegue l'inferenza via WebGPU con fallback WASM.

---

## 5. Layout del disco

Pochi file, grandi. Conseguenza diretta di V1 (exFAT) e V3 (enumerazione `webkitdirectory`).

```
/SwissBunker/
├── START.html               entry point Portable (self-contained, ~2 MB)
├── manifest.json            catalogo auto-descrittivo del disco
├── app/
│   ├── console.js           dashboard bundlata (classic script, no ES modules)
│   ├── reader.wasm          SQLite + lettori indici
│   └── assets/              font, icone, CSS
├── bin/
│   ├── darwin-arm64/swissbunkerd
│   ├── darwin-x64/swissbunkerd
│   ├── windows-x64/swissbunkerd.exe
│   └── linux-x64/swissbunkerd
├── content/
│   ├── wikipedia_it.zim
│   ├── wikipedia_en.zim
│   ├── gutenberg.zim
│   └── …                    un file monolitico per corpus
├── index/
│   ├── docstore.sqlite      testo dei chunk + metadati + mappatura alle fonti
│   ├── fts.sqlite           indice FTS5 (BM25)
│   ├── vectors.ivf          liste invertite: codici binari + residui int8
│   └── centroids.f32        centroidi IVF (caricati interamente in RAM)
├── models/
│   ├── embed/               embedder ONNX
│   ├── rerank/              cross-encoder ONNX
│   └── llm/                 pesi LLM per tier
├── personal/
│   └── vault.enc            area utente cifrata (AES-GCM)
└── .state/
    ├── journal.db           stato di download e build
    └── health.json          esito dell'ultimo health check
```

**`manifest.json`** è ciò che rende il disco auto-descrittivo: elenca corpora presenti,
versioni, date di snapshot, hash, statistiche degli indici, tier di modelli disponibili.
La dashboard lo legge in millisecondi e sa già tutto, senza scansionare 445 GB.

---

## 6. Formato dati e indici

Questa è la sezione dove si decide se il progetto funziona o no.

### 6.1 Il problema

Il corpus target contiene circa **110 GB di testo puro**. A chunk da ~500 caratteri sarebbero
**~220 milioni di vettori**: sull'M4 di riferimento, settimane di calcolo ininterrotto.
Inaccettabile.

### 6.2 La soluzione: asimmetria di costo

I due tipi di ricerca hanno costi di costruzione radicalmente diversi:

- **BM25 (SQLite FTS5)** — puro I/O e CPU. Nessuna GPU. Indicizza *tutti* i 110 GB in poche ore.
- **Vettoriale denso** — costa inferenza GPU per ogni singolo chunk.

Quindi la strategia è **copertura asimmetrica**:

| | Copertura | Chunk stimati |
|---|---|---|
| **BM25** | **100% del corpus** | ~220 M |
| **Denso** | Incipit di ogni articolo + libri interi + Q&A | **~30 M** |

Costo dell'embedding: ~30M chunk / ~2.000 chunk·s⁻¹ su M4 ≈ **4-5 ore**. Fattibile.

**Perché non si perde quasi nulla:** il vettoriale serve alle domande concettuali ("come si
purifica l'acqua senza filtri"), dove il lessico della domanda non coincide con quello del
testo. Per il match lessicale esatto BM25 è già imbattibile e copre tutto. Il recupero
denso sull'incipit basta a *identificare l'articolo giusto*; da lì BM25 e il reranker
lavorano sul testo completo.

### 6.3 Indice vettoriale: perché IVF e non HNSW

Decisione controintuitiva ma vincolante:

> **Su un file letto a range da disco esterno, IVF batte HNSW.**

HNSW naviga un grafo: centinaia di salti casuali **dipendenti** — ogni lettura determina la
successiva, quindi non si possono parallelizzare e si paga la latenza per intero.
Su USB, con ~0.3 ms per salto, sono centinaia di millisecondi di sola attesa.

IVF fa invece due cose sequenziali e prevedibili: confronta la query con i centroidi (già in
RAM), poi legge **`nprobe` liste invertite contigue**. Poche letture grandi invece di
centinaia piccole e seriali. È il pattern di accesso che un disco esterno ama.

> **Confermato in Fase 0 con un margine di 39×.** Sugli stessi 5.6 MB, da un file di 12.88 GB
> letto via `file://`: 1400 letture sparse da 4 KB costano 297.8 ms (p50), una singola
> lettura contigua da 5.6 MB ne costa 7.6. Vedi
> [findings Fase 0](../reports/2026-08-19-phase-0-findings.md) §3.1.
>
> Il lettore IVF è **sequenziale, non concorrente**: 32 letture parallele hanno battuto le
> stesse 32 sequenziali di appena 1.23×, un guadagno che non ripaga la complessità.

**Parametri:**
- `nlist` = 8192 cluster (~3.600 vettori per cluster)
- `nprobe` = 32 → ~117k candidati scansionati per query
- Codifica a due livelli: **binaria** (1 bit/dim → 48 byte/vettore) per lo scan,
  **int8** (384 byte/vettore) per il rescoring dei top-1000

**Budget:**

| Struttura | Dimensione | Dove vive |
|---|---|---|
| Centroidi | 12 MB | RAM |
| Codici binari (30M × 48 B) | 1.4 GB | RAM se disponibile, altrimenti mmap |
| Residui int8 (30M × 384 B) | 11.5 GB | Disco, letto solo per i top-1000 |

Letture per query: ~5.6 MB (scan binario) + ~384 KB (rescoring) → **I/O sotto i 30 ms** su
SSD USB3.

### 6.4 Perché SQLite e non Tantivy

Tantivy è più veloce a costruire, ma **non è leggibile dal browser**. SQLite compilato in
WASM con un VFS custom su `File.slice()` legge il database a pagine da 4 KB, esattamente come
farebbe da filesystem locale. È il vincolo del Runtime a decidere il formato dell'indice, non
le prestazioni della build.

> **Attenzione, verificato in Fase 0: `sql.js` non è compilato con FTS5.** Una query FTS5
> fallisce con `no such module: fts5` su tutti e quattro i motori.
>
> **Sostituito con `@sqlite.org/sqlite-wasm` 3.53.0** (SQLite ufficiale): FTS5 verificato con
> needle test, `sqlite3_vfs_register` esposto, wasm da 844 KB. Va bundlato in IIFE con il
> wasm passato via `wasmBinary`, perché il pacchetto è ESM. `sql.js-fts5` ha FTS5 ma **nessun
> VFS**, quindi caricherebbe l'intero indice in memoria: scartato. Vedi
> [findings Fase 0 §6bis](../reports/2026-08-19-phase-0-findings.md).
>
> **Confermato con un VFS funzionante** (Fase 1 Task 1, `web/spike-vfs/`): un VFS di sola
> lettura in ~105 righe risponde a una query FTS5 leggendo **32 KB su 1.5 MB, il 2.1% del
> database**. `wa-sqlite` ha un'API VFS più comoda e un wasm più piccolo, e il suo VFS
> funziona davvero — ma **nessuno dei suoi build contiene FTS5**, quindi è fuori.
>
> **Il VFS è però realizzabile**, il che prima della misura non era scontato: SQLite pretende
> letture *sincrone*, `File.slice()` è asincrono e `file://` nega `SharedArrayBuffer`, quindi
> il solito ponte worker + `Atomics.wait` non esiste. Una XHR **sincrona** contro il Blob URL
> di una porzione di file restituisce i byte corretti in **0.7 ms**, su tutti e quattro i
> motori. Vedi [findings Fase 0](../reports/2026-08-19-phase-0-findings.md) §3.2.

**Ottimizzazione di build:** l'indice FTS5 si costruisce sul disco interno veloce e si copia
sul disco esterno alla fine. Scrivere un indice SQLite direttamente su exFAT via USB è
patologicamente lento a causa del pattern di scrittura random del B-tree.

### 6.5 Strategia di chunking

- **Wikipedia:** chunk per sezione, con il titolo dell'articolo e la gerarchia delle sezioni
  ripetuti in testa a ogni chunk (contestualizzazione). Solo l'incipit va nell'indice denso.
- **Libri:** finestra scorrevole di ~1.200 caratteri con overlap del 15%, allineata ai
  confini di paragrafo, con titolo e capitolo in testa.
- **Q&A (Stack Exchange):** domanda + risposta accettata come unità atomica. Mai spezzate.
- **Manuali tecnici:** chunk per procedura/step, mai a metà di una sequenza operativa.

---

## 7. Pipeline di retrieval

```
   query utente
        │
        ├──────────────────────┬─────────────────────┐
        ▼                      ▼                     │
  espansione query      embedding query              │
  (sinonimi, lingua)    (ONNX, ~50 ms)               │
        │                      │                     │
        ▼                      ▼                     │
   BM25 / FTS5            IVF search                 │
   top-200                top-200                    │
        │                      │                     │
        └──────────┬───────────┘                     │
                   ▼                                 │
          Reciprocal Rank Fusion                     │
              top-100                                │
                   │                                 │
                   ▼                                 │
          Cross-encoder reranker  ◄──────────────────┘
              top-10                          (filtri utente:
                   │                       fonte, lingua, data)
                   ▼
        ┌──────────┴──────────┐
        ▼                     ▼
   risultati UI          contesto per LLM
                        (con ancore di citazione)
```

**Reciprocal Rank Fusion** invece della somma pesata dei punteggi: i punteggi BM25 e coseno
vivono su scale incomparabili e non normalizzabili in modo stabile fra corpora eterogenei.
RRF usa solo il **rango**, quindi è immune al problema. Formula: `score = Σ 1/(k + rank_i)`
con `k = 60`.

**Il reranker è opzionale e disattivabile.** Su hardware debole aggiunge 1-2 s. La UI espone
un interruttore "ricerca approfondita" invece di imporre l'attesa a tutti.

> **Vincolo misurato in Fase 1: ordinare per rilevanza non scala sui termini frequenti.**
> Su un indice da 6.22 GB, ordinare un termine presente in 35.575 documenti costa **2120 ms
> e 5984 letture**, contro 12.9 ms e 39 letture senza ordinamento — **164×**. `ORDER BY rank`
> non aiuta: FTS5 deve comunque assegnare un punteggio a ogni match prima che `LIMIT` ne
> scarti il 99.9%.
>
> Mitigazione adottata: `count(*)` costa ~41 letture, quindi si conta prima e sopra i 2000
> match si restituiscono i risultati in ordine di archiviazione, dichiarando `ranked: false`
> fino alla UI. Un risultato non ordinato significa "documenti che contengono la parola", non
> "i migliori", e presentarli allo stesso modo ingannerebbe chi non può verificare.
>
> **Conseguenza per la Fase 3:** la fusione RRF eredita lo stesso limite, perché anche lei
> ordina. Il ramo BM25 deve applicare il cutoff prima della fusione, non dopo. Vedi
> [verifica Fase 1 §2.2](../reports/2026-08-19-phase-1-verification.md).

---

## 8. Runtime LLM

### 8.1 Tiering hardware

Il sistema rileva memoria, presenza e classe di WebGPU, e sceglie il tier. Nessun modello
viene mai caricato se non c'è la certezza che entri.

> **Ritarata dopo la Fase 0.** `maxBufferSize` misurato su Apple M4 è **4.29 GB**, non i
> ~2 GB assunti: il Tier 3 è raggiungibile dove questa tabella si fermava al Tier 2. I valori
> qui sotto restano **stime di riferimento**; il tier effettivo si decide da `maxBufferSize` e
> dal limite di heap letti a runtime. Serve ancora la taratura su GPU integrata Intel/AMD e
> su GPU discreta.

| Tier | Modello | Peso | Requisito | Throughput atteso |
|---|---|---|---|---|
| T0 | Qwen3-0.6B q4 | ~0.4 GB | Nessun WebGPU, solo WASM | 3-6 tok/s |
| T1 | Qwen3-1.7B q4f16 | ~1.1 GB | WebGPU, ≥4 GB memoria | 8-15 tok/s |
| T2 | Qwen3-4B-Instruct q4f16 | ~2.5 GB | WebGPU, ≥8 GB memoria | 15-40 tok/s |
| T3 | Qwen3-8B q4f16 | ~5.0 GB | GPU discreta o Apple Silicon ≥16 GB | 25-60 tok/s |

**Tier 0 esiste per un motivo preciso:** su un PC senza WebGPU il sistema deve degradare a
"ricerca + riassunto lento", non rifiutarsi di funzionare. Un bunker che si apre solo su
hardware buono non è un bunker.

### 8.2 Caricamento dei pesi

Problema noto: le librerie di inferenza in-browser (WebLLM/MLC, wllama) sono progettate per
**scaricare** i pesi da HTTP e metterli in Cache API. Qui i pesi sono già sul disco e vanno
letti da oggetti `File`.

**Soluzione:** un adattatore che intercetta il loader e serve gli shard da `File.slice()`.
È un fork contenuto ma reale — tracciato come rischio R2.

### 8.3 Contratto di generazione

Il modello **non risponde mai da conoscenza parametrica**. Il prompt di sistema impone:

- ogni affermazione deve poggiare su un passaggio recuperato, marcato con un'ancora `[n]`
- se il contesto non contiene la risposta, la risposta è "non è nel bunker" più i risultati
  di ricerca più vicini
- le ancore sono cliccabili e aprono il passaggio esatto nella fonte originale

Questa è una scelta di prodotto, non tecnica: **in un contesto dove non puoi verificare
niente online, un'allucinazione è più dannosa di un "non lo so".**

---

## 9. La dashboard

### 9.1 Principi di design

1. **Una schermata, uno scopo.** Niente pannelli con sei funzioni.
2. **Il vuoto è progettato.** La schermata di un bunker vuoto deve dire cosa fare, non
   mostrare zeri.
3. **Onestà sui tempi.** Ogni operazione lunga dichiara una stima calcolata sulla velocità
   reale misurata, mai una barra che finge.
4. **Nessun gergo.** Mai `wikipedia_en_all_maxi_2026-06.zim`. Sempre "Wikipedia inglese, con
   immagini, 6.9M articoli, giugno 2026".
5. **Leggera per definizione.** Deve aprirsi su un portatile del 2015. Nessun framework
   pesante, nessuna animazione costosa, bundle sotto i 2 MB.

### 9.2 Flusso del bootstrap (terminale)

```
$ curl -fsSL https://swissbunker.sh | sh

  SwissBunker — bootstrap

  Dischi disponibili:

  ▸ 1) Samsung T7 Shield          1.8 TB liberi   USB 3.2   ●●●○ buono
    2) WD My Passport             3.6 TB liberi   USB 3.0   ●●○○ sufficiente
    3) Macintosh HD (interno)      168 GB liberi   NVMe      ●●●● ottimo
                                                   ↑ non consigliato: non è portatile

  Seleziona [1]: 1

  Verifico Samsung T7 Shield…
    Filesystem     exFAT ✓  (compatibile Windows/macOS/Linux)
    Velocità       lettura 980 MB/s · scrittura 850 MB/s ✓
    Spazio         1.8 TB liberi ✓

  Installo SwissBunker sul disco (147 MB)… ✓
  Avvio il daemon…                          ✓

  Apri la dashboard: http://localhost:7777
  (la apro io fra 3 secondi)
```

**Nota di sicurezza:** se il filesystem non è exFAT, il bootstrap **non formatta**. Spiega il
problema, indica le conseguenze (il disco funzionerà solo sul sistema operativo corrente) e
lascia decidere. Nessuna operazione distruttiva senza conferma esplicita e ripetuta.

### 9.3 Wizard di acquisizione (dashboard)

**Schermo 1 — Cosa ti serve?**

Sei preset, presentati come carte con una frase che spiega a chi servono:

| Preset | Contenuto | Peso |
|---|---|---|
| **Sopravvivenza** | Medicina, agricoltura, costruzione, riparazioni, radio, mappe | ~90 GB |
| **Studente** | Wikipedia IT+EN, manuali, matematica, scienze, Khan | ~150 GB |
| **Biblioteca** | Gutenberg, Wikisource, Wikibooks, dizionari | ~120 GB |
| **Tecnico / Maker** | Stack Exchange, iFixit, documentazione, datasheet | ~110 GB |
| **Bunker completo** | Tutto quanto sopra, deduplicato | ~335 GB |
| **Su misura** | Vai al catalogo | — |

Le dimensioni indicate sono i **contenuti scaricati**. Indici, modelli e applicazione
aggiungono ~90 GB al bunker completo, ~30-50 GB ai preset singoli. La barra dello Schermo 2
mostra sempre il **totale finale a disco**, non il download.

**Schermo 2 — Catalogo** (opzionale)

Ogni voce mostra: nome umano, lingua, dimensione, data di snapshot, numero di documenti, e
**tre esempi reali** di cosa contiene. In alto, sempre visibile, una barra:

```
  Disco: Samsung T7 Shield
  ████████████████████░░░░░░░░░░░░░░░  425 GB / 1.8 TB   (1.38 TB liberi dopo)
  335 GB da scaricare  +  90 GB di indici e modelli generati in locale

  Download stimato   5h 40m  (su 180 Mbps misurati)
  Indicizzazione     8h 15m  (stima su questo hardware)
```

Le voci che non entrano nello spazio residuo sono **disabilitate con la spiegazione**, non
selezionabili e poi fallimentari.

**Schermo 3 — Conferma**

Riepilogo, spazio, tempo, e un solo bottone. Con un avviso chiaro se il tempo stimato supera
le 12 ore.

**Schermo 4 — Avanzamento**

Una vista, tre fasi, sempre riprendibile:

```
  Costruzione del bunker                          ▸ in pausa   ▸ interrompi

  ✓  Scaricamento          335 GB / 335 GB          completato in 5h 12m
  ▶  Estrazione testo      241 GB / 335 GB   ████████████░░░░░  2h 04m rimanenti
  ○  Indicizzazione        in attesa
  ○  Verifica              in attesa

     In lavorazione: Stack Exchange — physics.stackexchange.com
     Velocità: 43 MB/s · Chunk creati: 18.4 M

     Puoi chiudere questa finestra. Il processo continua.
     Puoi scollegare il disco solo dopo aver premuto "interrompi".
```

**Schermo 5 — Health check**

Alla fine il sistema **si testa da solo** e mostra i risultati con le prove:

```
  Il tuo bunker funziona ✓

  Ricerca lessicale       "penicillina"          847 risultati    124 ms
  Ricerca concettuale     "come depurare acqua"   top-10 rilevanti  310 ms
  Lettura fonte           Wikipedia IT → aperta                     89 ms
  Modello locale          Qwen3-4B su Apple M4    34 tok/s
  RAG end-to-end          domanda → risposta con 4 citazioni       2.1 s

  Contiene: 8.2M articoli · 71k libri · 4.1M domande e risposte
            Europa in mappe · 30.2M passaggi indicizzati
```

Questa schermata è il momento in cui l'utente capisce di possedere qualcosa che funziona.
Nomad non ha nulla di equivalente, ed è una delle ragioni per cui lascia addosso la
sensazione di non sapere se si è fatto giusto.

### 9.4 Dashboard in uso

- **Cerca** — barra unica, risultati raggruppati per fonte, filtri laterali, anteprima inline.
- **Chiedi** — chat RAG con citazioni cliccabili; ogni citazione apre il passaggio esatto.
- **Sfoglia** — navigazione per corpus, come una biblioteca.
- **Mappe** — visualizzatore Protomaps offline.
- **Il mio archivio** — area personale cifrata, con import drag&drop.
- **Il bunker** — stato, contenuti, aggiornamenti (solo in modalità Connected).

---

## 10. Sicurezza e privacy

- **Contenuti pubblici in chiaro.** È Wikipedia; cifrarla costerebbe prestazioni per zero
  beneficio.
- **Area personale cifrata lato client.** AES-GCM 256 via WebCrypto, chiave derivata da
  passphrase con Argon2id. **La cifratura la fa il browser**, quindi resta compatibile con il
  vincolo "zero eseguibili" del Runtime. WebCrypto è disponibile perché `file://` risulta
  `isSecureContext: true` su tutti i motori testati.
- **Lo storage cifrato usa IndexedDB, non OPFS** (vincolo V7): OPFS è negato da `file://` su
  Chromium, Chrome e WebKit, mentre IndexedDB passa su tutti e quattro.
- **Nessuna telemetria.** Mai. Il daemon non apre connessioni in uscita se non per download
  espressamente richiesti.
- **Binding su loopback.** Il daemon ascolta solo su `127.0.0.1`, mai su `0.0.0.0`.
- **Catalogo firmato.** Il file di catalogo è firmato; il daemon rifiuta cataloghi non
  verificati, per impedire il redirect dei download verso mirror ostili.
- **Verifica hash obbligatoria** su ogni corpus scaricato.

---

## 11. Budget

### 11.1 Spazio (target 1 TB)

| Voce | Scelta | Dimensione |
|---|---|---|
| Wikipedia IT | `all_maxi` (con immagini) | 40 GB |
| Wikipedia EN | `all_nopic` | 55 GB |
| Gutenberg + Wikisource/books/dizionari | completi | 95 GB |
| Stack Exchange | top ~50 siti | 60 GB |
| iFixit, WikiMed, Khan, manuali pratici | completi | 40 GB |
| Mappe OSM (Protomaps) | Europa | 35 GB |
| arXiv | abstract + metadati | 10 GB |
| **Sottototale contenuti** | | **335 GB** |
| Indice FTS5 | ~40% del testo | 45 GB |
| Docstore (chunk + metadati) | | 20 GB |
| Indice vettoriale (binario + int8) | | 13 GB |
| Modelli (embed + rerank + 4 tier) | | 12 GB |
| Applicazione e binari | | 0.2 GB |
| **Totale** | | **~425 GB** |

Restano ~575 GB liberi su 1 TB. Upgrade successivi possibili: Wikipedia EN `all_maxi`
(+55 GB), OSM planet (+85 GB), Stack Exchange completo (+40 GB).

### 11.2 Tempo di costruzione (M4 base, 16 GB)

| Fase | Stima | Collo di bottiglia |
|---|---|---|
| Download | 5-7 h | Banda internet |
| Estrazione testo | 2-3 h | CPU + I/O USB |
| Chunking | 1 h | CPU |
| Indice FTS5 | 3-4 h | I/O (su disco interno, poi copia) |
| Embedding 30M chunk | 4-5 h | GPU |
| Costruzione IVF | 1 h | CPU + RAM |
| Verifica | 10 min | — |
| **Totale** | **16-21 h** | riprendibile in ogni momento |

---

## 12. Stack tecnologico

| Livello | Scelta | Perché |
|---|---|---|
| Daemon | **Rust** (axum, tokio, rusqlite, ort) | Binario statico senza runtime; unica lingua che dà un eseguibile copiabile su exFAT che parte ovunque |
| Full-text | **SQLite FTS5** via `@sqlite.org/sqlite-wasm` 3.53.0 | L'unico indice testuale serio leggibile dal browser via WASM. Scelto per il VFS, non solo per FTS5: è l'unico candidato che espone `sqlite3_vfs_register`. **Non `sql.js`** (niente FTS5) né `sql.js-fts5` (niente VFS) |
| Vettoriale | **IVF custom** (formato proprio) | Nessuna libreria esistente ha un formato pensato per il range-read da browser |
| Embedder | **multilingual-e5-small** ONNX | 384 dim, multilingue IT/EN, ~2k chunk/s su M4 |
| Reranker | **jina-reranker-v2-base-multilingual** ONNX q8 | Buon rapporto qualità/peso per l'esecuzione in browser |
| LLM | **WebLLM/MLC** con loader custom | Unico stack maturo per WebGPU in browser |
| Contenuti | **ZIM** (formato Kiwix) | Standard de facto, monolitico, compresso, con indice interno |
| Mappe | **Protomaps PMTiles** | Un solo file, range-read nativo, perfetto per il vincolo |
| Dashboard | **TypeScript + Lit** (no React) | Bundle minuscolo, Web Components, funziona da `file://` come script classico |
| Crypto | **WebCrypto** nativa | Zero dipendenze, disponibile ovunque |

---

## 13. Fasi di progetto

### Fase 0 — Spike di fattibilità *(1 settimana) — GATE GO/NO-GO*

**L'unica fase il cui output è una risposta, non del codice.** Verifica empirica dei vincoli
V2, V3, V4 su Chrome, Edge, Safari 26, Firefox 147, macOS e Windows:

1. Una pagina aperta da `file://` può leggere una directory con `webkitdirectory`?
2. `File.slice()` funziona su un file da 40 GB su exFAT? Qual è la latenza reale?
3. SQLite-WASM con VFS custom interroga un database da 10 GB via `slice()`?
4. WebGPU è accessibile da `file://` (origin nullo)?
5. I Web Worker via Blob URL funzionano da `file://`?
6. Un modello WebLLM carica i pesi da oggetti `File` invece che da HTTP?

**Se i punti 1-4 falliscono su tutti i browser**, la modalità Portable è morta nella forma
attuale e si ripiega sul piano B (sezione 14, R1). Questa fase va fatta **prima di ogni altra
riga di codice**. Tutto il codice scritto qui è usa-e-getta.

### Fase 1 — Reader (2-3 settimane)
VFS SQLite su `File.slice()`, lettore ZIM, ricerca FTS5, UI risultati minimale.
**Criterio di uscita:** cercare su una Wikipedia IT reale da `file://` in meno di 500 ms.

### Fase 2 — Bootstrap e daemon (3 settimane)
One-liner, selezione disco, benchmark, catalogo, download manager riprendibile, build FTS5.
**Criterio di uscita:** `curl | sh` → wizard → Wikipedia IT scaricata, indicizzata, cercabile.

### Fase 3 — Pipeline vettoriale (3 settimane)
Chunking per tipo di corpus, embedding via ONNX, costruzione IVF, RRF, reranking.
Include la **costruzione del golden set**: 200 domande con i passaggi corretti annotati a
mano, distribuite fra i corpora e fra query lessicali e concettuali. Senza questo insieme di
riferimento i requisiti NF2 e NF4 non sono misurabili, e ogni modifica al retrieval diventa
una scommessa.
**Criterio di uscita:** Recall@10 > 0.85 sul golden set; p95 < 800 ms.

### Fase 4 — LLM in-browser (2-3 settimane)
Rilevamento hardware, tiering, loader dei pesi da `FileList`, RAG con citazioni verificabili.
**Criterio di uscita:** > 10 tok/s su Tier 2 con citazioni corrette al 100%.

### Fase 5 — Console UX (3 settimane)
Design system, wizard completo, viste di avanzamento, health check, archivio personale.
**Criterio di uscita:** una persona non tecnica costruisce un bunker senza assistenza.

### Fase 6 — Hardening e rilascio (2-3 settimane)
Cifratura, update differenziali, matrice cross-browser, firma dei binari, documentazione.
**Criterio di uscita:** tutti i requisiti NF verificati su 3 sistemi operativi.

**Totale: 16-20 settimane part-time.**

---

## 14. Registro rischi

| ID | Rischio | Impatto | Prob. | Evidenza | Mitigazione / Piano B |
|---|---|---|---|---|---|
| **R1** | `file://` blocca troppe API e la modalità Portable è irrealizzabile | Critico | **Neutralizzato** | [F0 §1](../reports/2026-08-19-phase-0-findings.md) — `webkitdirectory` PASS su 4/4 motori | Residuo su ESM/fetch: tutto bundlato IIFE, wasm inlineato in base64 |
| **R2** | WebLLM non caricabile da `FileList` senza fork sostanziale | Alto | **Bassa** | [F0 §1](../reports/2026-08-19-phase-0-findings.md) — `cache_api_accepts_file` e `file_transfers_to_worker` PASS su 4/4 | Pre-popolare la Cache API per soddisfare il loader della libreria |
| **R3** | Corruzione exFAT su file da decine di GB / scritture lunghe | Alto | Bassa | Non misurato | Journal transazionale, checksum per corpus, rename atomico |
| **R4** | Throughput embedding sull'M4 sotto le attese | Medio | Media | Non misurato | Ridurre la copertura densa; BM25 copre comunque il 100% |
| **R5** | Reranker troppo lento in browser su hardware debole | Basso | Alta | Non misurato | Già previsto come interruttore opzionale |
| **R6** | `webkitdirectory` fallisce con file > 4 GB | Medio | **Neutralizzato** | [F0 §1](../reports/2026-08-19-phase-0-findings.md) — file da 12.88 GB letto in coda su 4/4 | — |
| **R7** | Limite `maxBufferSize` WebGPU inferiore al previsto | Medio | **Rovesciato** | [F0 §3.3](../reports/2026-08-19-phase-0-findings.md) — 4.29 GB su Apple M4, non ~2 GB | Il vincolo era troppo conservativo; tier deciso a runtime |
| **R8** | Banda internet insufficiente rende la build impraticabile | Medio | Bassa | Non misurato | Il wizard misura la banda e avvisa prima; supporto torrent |
| **R9** | Mirror Kiwix lenti o irraggiungibili | Medio | Media | Il download ZIM di prova è fallito in Fase 0 (URL con suffisso data) | Mirror multipli, torrent primario, cataloghi versionati |
| **R10** | `sql.js` non include FTS5 | Alto | **Risolto** | [F0 §6bis](../reports/2026-08-19-phase-0-findings.md) — `@sqlite.org/sqlite-wasm` 3.53.0 ha FTS5 e `sqlite3_vfs_register` | Adottato l'ufficiale. Resta da provare il percorso completo da `file://` su DB grande: è il criterio di uscita della Fase 1 |
| **R11** | **OPFS non disponibile da `file://`** su Chromium e WebKit | Medio | **Certa** | [F0 §2](../reports/2026-08-19-phase-0-findings.md) | Storage utente su IndexedDB (PASS su 4/4). Vincolo V7 |
| **R12** | **WebGPU non verificato su Safari e Firefox reali** | Medio | Aperta | Le build Playwright dei due motori non spediscono WebGPU | Verifica manuale obbligatoria prima della Fase 4 |
| **R13** | Il lettore pigro non ha politica di eviction | Basso | **Certa** | [F0 §1](../reports/2026-08-19-phase-0-findings.md) — 795 MB di cache contro soglia 500 MB | LRU e chunk più piccolo di 1 MB: l'amplificazione a 39.7 MB/query è dominata dal chunk |

---

## 15. Criteri di accettazione

Il progetto è completo quando, su tre macchine mai viste prima (Windows, macOS, Linux):

1. Il disco si collega e la dashboard si apre in meno di 90 secondi, senza installare nulla.
2. Una ricerca lessicale restituisce risultati in meno di 800 ms (p95).
3. Una domanda in linguaggio naturale produce una risposta con citazioni verificabili.
4. Ogni citazione apre il passaggio esatto nella fonte originale.
5. Recall@10 sul golden set di 200 domande supera 0.85.
6. Il sistema degrada con grazia su hardware senza WebGPU, invece di rifiutarsi di partire.
7. Non è stato richiesto alcun privilegio amministrativo.
8. Staccando il disco a metà di una build e ricollegandolo, il processo riprende.

---

## 16. Decisioni aperte

Punti dove serve una scelta di prodotto, da fissare prima delle rispettive fasi:

- **Politica di tiering** (Fase 4) — quando l'hardware è al confine fra due tier, si sceglie
  il modello più grande (risposte migliori, rischio di esaurire memoria) o il più piccolo
  (sempre funzionante, risposte più deboli)? Il comportamento al limite va deciso
  esplicitamente, non lasciato all'euristica.
- **Profondità dell'indice denso** (Fase 3) — solo l'incipit degli articoli, oppure anche le
  sezioni principali? Ogni livello in più costa ore di build e GB di indice.
- **Interfaccia di scoperta** (Fase 5) — la home è una barra di ricerca vuota o una vista
  che invoglia a esplorare ciò che si possiede? Cambia il carattere del prodotto.
- **Aggiornamenti** (Fase 6) — automatici quando il disco è connesso a una rete, o sempre
  manuali? Un bunker che si aggiorna da solo è comodo ma smette di essere prevedibile.
