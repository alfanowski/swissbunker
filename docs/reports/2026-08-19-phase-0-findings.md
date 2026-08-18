# Fase 0 — Findings e decisione

**Data:** 2026-08-19
**Esecuzioni:** 48 record · 6 probe · 4 motori · 2 protocolli · 1 sistema operativo
**Hardware:** Apple M4, 16 GB, macOS · immagine exFAT sparse da 120 GB
**Decisione:** **GO CON MODIFICHE**

---

## Sommario

**La modalità Portable è realizzabile.** Su tutti e quattro i motori testati, una pagina
aperta da `file://` enumera una directory da 19 GB, ottiene oggetti `File` validi e legge
correttamente byte arbitrari da un file da 12.88 GB — che è la scommessa su cui poggia
l'intera architettura. Il rischio R1 passa da *Media* a **Neutralizzato** per la parte che
conta.

Il vincolo più stringente emerso non era nel registro rischi: **`sql.js` non è compilato con
FTS5**, quindi il motore full-text scelto nella spec §6.4 non esiste nella libreria data per
scontata. È una flag di compilazione, non un muro, ma cambia il lavoro della Fase 1.

In compenso è caduto il blocco teorico peggiore: **una lettura sincrona e bloccante da un
`File` è possibile in 0.7 ms**, su tutti e quattro i motori, senza `SharedArrayBuffer`. Un
VFS SQLite vero è quindi implementabile, cosa che prima della misura era in dubbio.

---

## 1. Verdetti per rischio

| Rischio | Stimato nella spec | Esito misurato | Nuova valutazione |
|---|---|---|---|
| **R1** — `file://` blocca troppe API | Media, impatto Critico | `webkitdirectory` PASS su 4/4 motori da `file://`; lettura di `File` corretta | **Neutralizzato** per la lettura del disco; residuo su ESM/fetch/OPFS, tutti aggirabili |
| **R2** — pesi LLM da `FileList` | Media, impatto Alto | `cache_api_accepts_file` PASS 4/4; `file_transfers_to_worker` PASS 4/4 con size corretto su 491 MB | **Bassa** — esiste il percorso economico previsto |
| **R6** — `webkitdirectory` oltre 4 GB | Media, impatto Medio | File da 12.88 GB enumerato e letto in coda su 4/4 | **Neutralizzato** |
| **R7** — limite `maxBufferSize` WebGPU | Media, impatto Medio | 4.29 GB su Apple M4, non i ~2 GB assunti | **Rovesciato**: la spec era troppo conservativa. Da confermare su hardware non-Apple |

### Rischi nuovi, non presenti nella spec

| ID | Rischio | Impatto | Evidenza |
|---|---|---|---|
| **R10** | `sql.js` non include FTS5 | Alto → **risolto**, vedi §6bis | `fts5_engine_works` → `no such module: fts5` su 4/4 motori |
| **R11** | OPFS non disponibile da `file://` su Chromium e WebKit | Medio | `opfs_usable` fail su chromium/chrome/webkit, PASS su firefox |
| **R12** | WebGPU non verificabile su Safari e Firefox reali con questa toolchain | Medio | Le build Playwright di Firefox e WebKit non spediscono WebGPU |
| **R13** | Il lettore pigro non ha politica di eviction | Basso | `cache_stayed_bounded` fail: 795 MB di cache, oltre la soglia di 500 MB |

---

## 2. Cosa `file://` toglie davvero

Confronto diretto fra la condizione reale e il controllo `http://`, su Chromium:

| Capacità | `http://` | `file://` | Conseguenza |
|---|---|---|---|
| `<script src>` classico | PASS | **PASS** | La via di caricamento del codice resta aperta |
| Import dinamico di moduli ES | PASS | fail | Previsto. Tutto va bundlato in IIFE |
| `fetch` di un file vicino | PASS | fail | Previsto. Il wasm va inlineato in base64 |
| `WebAssembly.instantiate` da byte inline | PASS | **PASS** | La via d'uscita al punto sopra funziona |
| Script via Blob URL | PASS | **PASS** | La via d'uscita generale funziona |
| `localStorage` | PASS | **PASS** | Impostazioni utente possibili |
| `IndexedDB` | PASS | **PASS** | Dati utente possibili su tutti e 4 i motori |
| OPFS | PASS | fail | **Non previsto.** Rompe il piano per F8/F9 |
| Worker via Blob URL | PASS | **PASS** | Il lavoro pesante può lasciare il thread principale |

`file://` risulta `isSecureContext: true` su tutti i motori, il che è la ragione per cui
WebGPU e WebCrypto restano disponibili. `crossOriginIsolated` è `false` e
`SharedArrayBuffer` assente, come atteso: nessun WASM multi-thread su CPU.

---

## 3. Le misure che decidono il design

### 3.1 IVF contro HNSW — confermato con un margine di 39×

La spec §6.3 sceglie IVF sostenendo che i salti casuali e dipendenti di HNSW sono
latency-bound su un file letto a range. Misurato su un file da 12.88 GB, `file://`, Chromium:

| Pattern | Byte letti | p50 | p95 |
|---|---|---|---|
| 1400 letture sparse da 4 KB (traversata HNSW) | 5.6 MB | **297.8 ms** | 306.6 ms |
| 1 lettura contigua da 5.6 MB (scan IVF `nprobe`=32) | 5.6 MB | **7.6 ms** | 39.0 ms |

**Stessi byte, 39.2 volte il costo.** La scelta architetturale non era una preferenza di
stile: è la differenza fra stare dentro il budget NF2 di 800 ms e mangiarselo tutto in
retrieval.

Letture parallele contro sequenziali (32 × 256 KB): 22.7 ms contro 27.9 ms, appena
**1.23×**. Il lettore IVF può restare sequenziale; la concorrenza non ripaga la complessità.

### 3.2 Lettura sincrona bloccante — il blocco teorico non esiste

Il problema: un VFS SQLite pretende letture **sincrone**, `File.slice()` è asincrono, e
`file://` nega `SharedArrayBuffer`, quindi il solito ponte worker + `Atomics.wait` non è
disponibile.

Misurato: una XHR **sincrona** contro il Blob URL di una porzione di file restituisce
`status 200`, esattamente 4096 byte corretti, in **0.7 ms**. PASS su tutti e quattro i
motori, in entrambi i protocolli.

Questo sblocca l'intero motore di ricerca. È il risultato più importante della Fase 0.

### 3.3 Altre misure

| Misura | Valore | Budget | Margine |
|---|---|---|---|
| Enumerazione di 7 file / 19.23 GB (`webkitdirectory`) | **0 ms** | — | Il design a file monolitici ripaga |
| Lettura casuale singola da 4 KB | 0.20 ms p50 | — | — |
| Query FTS5 simulata su DB da 6.22 GB | 100.7 ms p50 / 159.3 p95 | NF2: 800 ms | Ampio |
| Streaming di 491 MB di pesi in chunk da 64 MB | 112.5 ms (~4.4 GB/s) | NF1: 90 s | Ampissimo |
| Limite heap JS | 3760 MB | V4: ~4 GB | Confermato |
| `maxBufferSize` WebGPU (Apple M4) | 4.29 GB | V4: ~2 GB | **Vincolo troppo conservativo** |

**Avvertenza sulle latenze:** tutte le misure di I/O provengono dall'immagine exFAT su NVMe
interno. Riproducono la semantica di exFAT, **non** la latenza USB. Su disco esterno i valori
assoluti peggioreranno; il rapporto 39× fra i due pattern dovrebbe invece migliorare a favore
di IVF, perché il costo per singola operazione cresce e le 1400 letture seriali ne pagano 1400.

---

## 4. Conferme e smentite delle scelte architetturali

- **`webkitdirectory` invece di File System Access — confermato e necessario.**
  `showDirectoryPicker` è assente su Firefox e WebKit, presente su Chromium e Chrome.
  `webkitdirectory` passa su tutti e quattro. Dipendere dalla File System Access API avrebbe
  tagliato fuori metà dei motori.
- **File monolitici — confermato.** Sette file per 19.23 GB si enumerano in tempo non
  misurabile. Il vincolo exFAT e il vincolo browser puntavano davvero nella stessa direzione.
- **IVF invece di HNSW — confermato, 39×.**
- **SQLite FTS5 come motore full-text — confermato come scelta, smentito come libreria.**
  Il motore va bene; `sql.js` non lo contiene. Sostituito con `@sqlite.org/sqlite-wasm`
  3.53.0 (§6bis).
- **Tiering dei modelli — da ritarare.** Con 4.29 GB di buffer su Apple Silicon, il Tier 3
  è raggiungibile dove la spec si fermava al Tier 2.
- **OPFS per l'area personale — smentito.** Va sostituito con IndexedDB.

---

## 5. Modifiche richieste alla specifica

| Sezione | Modifica |
|---|---|
| §3.3 V4 | `maxBufferSize` non è ~2 GB: misurato 4.29 GB su Apple M4. Riformulare come "variabile per GPU, da rilevare a runtime", mai come costante |
| §3.3 (nuovo V7) | Aggiungere: OPFS non è disponibile da `file://` su Chromium e WebKit. Lo storage utente in modalità Portable usa IndexedDB |
| §6.4 | Sostituire `sql.js` con un build di SQLite WASM che includa FTS5. Candidati: `@sqlite.org/sqlite-wasm` (ufficiale, FTS5 di serie, API VFS), `wa-sqlite` (pensato per VFS custom), `sql.js-fts5` |
| §6.3 | Annotare il rapporto 39× come evidenza a supporto; specificare che il lettore IVF è **sequenziale**, non concorrente (guadagno misurato solo 1.23×) |
| §8.1 | Ritarare la tabella dei tier: il Tier 3 è plausibile su Apple Silicon. Il tier va deciso da `maxBufferSize` letto a runtime, non da una tabella statica |
| §9.4 / F8 / F9 | L'area personale cifrata usa IndexedDB, non OPFS. WebCrypto resta valida: `file://` è secure context |
| §12 | Nella tabella dello stack, sostituire la riga `sql.js` |
| §14 | Aggiungere R10, R11, R12, R13; aggiornare R1, R2, R6, R7 con le probabilità misurate |

---

## 6. Limiti di questo spike — cosa NON è stato dimostrato

Elencati per intero, perché un report che nasconde i propri buchi è peggio di nessun report.

1. **Un solo sistema operativo.** Solo macOS. Windows non è stato toccato, e i criteri di
   uscita della Fase 0 lo richiedono.
2. **Motori Playwright, non browser reali.** Chrome è stato testato nella build reale
   dell'utente; Chromium, Firefox e WebKit sono build Playwright. **Playwright WebKit non è
   Safari**: condivide il motore, non lo strato di policy del browser.
3. **WebGPU non verificato su Safari e Firefox.** Le build Playwright di Firefox e WebKit non
   spediscono WebGPU, quindi i loro `fail` sono un limite dello strumento, **non una prova**
   che Safari 26 e Firefox 147 non lo supportino — la documentazione dice il contrario. Questo
   buco va chiuso a mano prima della Fase 4.
4. **Nessuna misura su disco esterno reale.** Tutte le latenze vengono da un'immagine exFAT
   su NVMe interno. I numeri assoluti non sono trasferibili.
5. **Nessuna query FTS5 vera su file grande.** Il DB piccolo dimostra il percorso di
   caricamento, non il motore, perché il motore mancava del tutto.
6. **Una sola macchina.** Un Apple M4. Il tiering ha bisogno di almeno una GPU integrata
   Intel/AMD e una discreta per essere tarato.

### Contaminazione trovata e corretta

Nella prima passata, Firefox riportava 8/8 su `file://`, incluso l'import di moduli ES.
Il motivo: la build Playwright di Firefox spedisce
`pref("security.fileuri.strict_origin_policy", false)` — disabilita esattamente la policy
che questo spike misura. **Quei record erano fiction.** Il runner ora ripristina la
preferenza a `true`; Firefox scende a 6/8, in linea con gli altri motori. I record
contaminati sono stati cancellati, non corretti.

Chromium e WebKit sono stati verificati: Playwright non passa loro
`--allow-file-access-from-files` né `--disable-web-security`. I loro record sono puliti.

### Falso positivo trovato e corretto

`webgpu_inside_blob_worker` registrava PASS su Firefox e WebKit con dettaglio
`"no navigator.gpu"`. Causa: `Probe.check` tratta qualunque valore non falso come successo,
e il check restituiva la *ragione* del fallimento, che è una stringa vera. Corretto per
restituire `false`. Sarebbe stato un falso positivo su "l'inferenza può girare in un worker",
esattamente il genere di errore che questo spike esiste per evitare.

---

## 6bis. Addendum — R10 risolto (2026-08-19)

Valutati in Node i due candidati praticabili. Entrambi superano il needle test: una tabella
`fts5` creata, il token piantato in una riga su due, e la riga giusta restituita.

| | `@sqlite.org/sqlite-wasm` 3.53.0 | `sql.js-fts5` 1.4.0 |
|---|---|---|
| FTS5 | **sì** — needle trovato | **sì** — needle trovato |
| `sqlite3_vfs_register` | **presente** | assente |
| `FS.createLazyFile` / `registerDevice` / `mount` | n/d (usa l'API C) | **tutti `undefined`** |
| Dimensione wasm | **844 KB** | 1185 KB |
| Formato | ESM — va bundlato in IIFE | UMD — si carica da script tag |
| Manutentore | **il progetto SQLite** | fork di terze parti |

**Scelto: `@sqlite.org/sqlite-wasm`.** La differenza decisiva non è FTS5, che entrambi
hanno: è il VFS. `sql.js-fts5` non espone alcun meccanismo di lettura pigra, quindi
caricherebbe in memoria l'intero indice da decine di gigabyte — cioè esattamente ciò che il
progetto non può fare. Il suo unico vantaggio, caricarsi da uno script tag, riguarda un
problema che sappiamo già risolvere: bundling IIFE con il wasm passato via `wasmBinary`, la
stessa tecnica già usata in P4.

`wa-sqlite` non è stato valutato: l'ufficiale offre già VFS e FTS5 ed è mantenuto dal
progetto SQLite, quindi un terzo candidato non cambierebbe la decisione.

**Resta da verificare in Fase 1:** che il pacchetto ufficiale, bundlato in IIFE, registri un
VFS e completi una query FTS5 su database multi-gigabyte **da `file://`**. Questa prova
richiede un browser e non è stata fatta qui.

**R10 scende da Alto/Certa a Basso/Aperto.**

---

## 7. Cosa serve prima della Fase 1

1. ~~Scegliere il build di SQLite WASM con FTS5~~ — **fatto**, vedi §6bis. Resta da provare
   il percorso completo da `file://` su database grande, che è il criterio di uscita della
   Fase 1 stessa.
2. Ripetere P2, P3 e P4 su un **disco esterno exFAT reale** per ottenere il budget di latenza
   vero.
3. Ripetere l'intera batteria su **Windows**, con Chrome, Edge e Firefox.
4. Verificare WebGPU su **Safari 26 e Firefox 147 reali**, a mano.
5. Aggiungere una politica di eviction LRU al lettore pigro e ridurre il chunk da 1 MB: con
   39.7 MB letti per query, l'amplificazione è dominata dalla dimensione del chunk, non dal
   pattern di accesso.
6. Applicare alla spec le otto modifiche della sezione 5.

---

## 8. Decisione

**GO CON MODIFICHE.**

L'architettura Portable regge: la lettura del disco da `file://` funziona su ogni motore
testato, la lettura sincrona bloccante — che era il blocco teorico peggiore — è possibile in
meno di un millisecondo, e la scelta IVF è confermata da un margine di 39×.

Le modifiche non sono opzionali: la spec va aggiornata con gli otto punti della sezione 5
**prima** che inizi la Fase 1, in particolare la sostituzione di `sql.js`, che cambia il
lavoro della prima fase implementativa.

I punti 2, 3 e 4 della sezione 7 restano aperti e vanno chiusi prima delle fasi che ne
dipendono — non prima della Fase 1, che può partire su questi dati.
