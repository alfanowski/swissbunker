# Fase 0 — Spike di fattibilità · Piano di implementazione

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stabilire con misure riproducibili se la modalità Portable di SwissBunker — una
dashboard aperta da `file://` che legge indici multi-gigabyte da un disco exFAT e fa
inferenza LLM via WebGPU — è realizzabile, e produrre una decisione go/no-go documentata.

**Architecture:** Sei probe indipendenti, ciascuno una pagina HTML autonoma eseguita
identicamente da `file://` e da `http://localhost` (baseline di controllo). Ogni probe scrive
un record JSON con esito e misure; un aggregatore li fonde in una matrice
browser × sistema operativo. Nessun codice di questa fase entra in produzione.

**Tech Stack:** HTML/JS vanilla senza build step, `esbuild` per i bundle IIFE, `sql.js`,
`hdiutil` per immagini exFAT, Python `http.server` come baseline di controllo.

**Spec:** [`docs/specs/2026-08-18-swissbunker-design.md`](../specs/2026-08-18-swissbunker-design.md) — §13 Fase 0, §3.3 vincoli V2/V3/V4, §14 rischi R1/R2/R6/R7

---

## Global Constraints

- **Tutto il codice di questa fase è usa-e-getta.** Vive in `spikes/phase-0/`, non viene mai
  importato da codice di produzione, e non definisce interfacce per le fasi successive.
- **Nessun bundler, nessun framework, nessun `node_modules` nei probe.** I probe devono
  girare aprendo un file, senza alcun passaggio di build, perché è esattamente la condizione
  che stiamo misurando. `esbuild` è ammesso solo per pre-costruire bundle IIFE di librerie
  di terze parti, e l'output va committato.
- **Ogni probe gira in due condizioni:** `file://` (la condizione reale) e
  `http://localhost:8000` (il controllo). Un fallimento conta solo se `file://` fallisce
  dove `http://` riesce — altrimenti il problema è il codice del probe, non la piattaforma.
- **Browser obbligatori:** Chrome ≥ 113, Edge ≥ 113, Safari ≥ 26, Firefox ≥ 147.
  Sistemi operativi obbligatori: macOS e Windows.
- **Ogni misura di latenza è la mediana di ≥ 20 ripetizioni**, riportata con p50 e p95.
  Una singola misura non è un dato.
- **Nessun probe scrive fuori da `spikes/phase-0/results/`.**
- Codice, identificatori e commenti in inglese. Documentazione di piano in italiano.

---

## Struttura dei file

| File | Responsabilità |
|---|---|
| `spikes/phase-0/README.md` | Come eseguire lo spike, in che ordine, su quali macchine |
| `spikes/phase-0/lib/probe.js` | Runtime condiviso: misurazione, formattazione, export del record JSON |
| `spikes/phase-0/lib/file-vfs.js` | Lettore pigro che traduce le richieste di pagina in `File.slice()` |
| `spikes/phase-0/tools/make-fixtures.sh` | Genera immagine exFAT e database SQLite di test |
| `spikes/phase-0/tools/serve.sh` | Avvia il baseline `http://localhost:8000` |
| `spikes/phase-0/tools/build-vendor.sh` | Produce i bundle IIFE delle librerie di terze parti |
| `spikes/phase-0/p1-module-loading.html` | P1 — quali forme di caricamento codice sopravvivono a `file://` |
| `spikes/phase-0/p2-directory-picker.html` | P2 — enumerazione directory e accesso ai `File` |
| `spikes/phase-0/p3-range-read.html` | P3 — correttezza e latenza di `File.slice()` su file multi-GB |
| `spikes/phase-0/p4-sqlite-lazy.html` | P4 — query SQLite su database grande letto a range |
| `spikes/phase-0/p5-webgpu-worker.html` | P5 — WebGPU e Web Worker sotto origin nullo |
| `spikes/phase-0/p6-model-weights.html` | P6 — caricamento pesi LLM da oggetti `File` |
| `spikes/phase-0/report.html` | Aggregatore: fonde i record JSON nella matrice finale |
| `spikes/phase-0/results/` | Record JSON prodotti dalle esecuzioni (committati) |
| `docs/reports/2026-XX-XX-phase-0-findings.md` | Il deliverable: esiti, matrice, decisione go/no-go |

**Perché ogni probe è un file HTML separato** e non un'unica pagina con sei sezioni: un
probe che manda in crash la scheda (P6 satura la memoria per costruzione) non deve portarsi
via i risultati degli altri cinque. L'isolamento qui è una scelta di robustezza
sperimentale, non di stile.

---

### Task 0: Harness e fixture

**Files:**
- Create: `spikes/phase-0/README.md`
- Create: `spikes/phase-0/lib/probe.js`
- Create: `spikes/phase-0/tools/make-fixtures.sh`
- Create: `spikes/phase-0/tools/serve.sh`
- Create: `spikes/phase-0/results/.gitkeep`

**Interfaces:**
- Consumes: niente
- Produces: `window.Probe`, con le funzioni usate da ogni probe successivo:
  - `Probe.init(id: string, title: string): object`
  - `Probe.check(name: string, fn: () => any | Promise<any>): Promise<{ok: boolean, detail: any}>` — esegue un controllo booleano, cattura le eccezioni e le trasforma in un fallimento con messaggio invece di interrompere la pagina
  - `Probe.measure(name: string, fn: (i: number) => Promise<any>, runs?: number): Promise<{runs: number, p50: number, p95: number, min: number, max: number}>` — latenza in millisecondi
  - `Probe.info(key: string, value: any): void` — annota un dato di contesto
  - `Probe.finish(): object` — mostra il riepilogo e abilita il download del record JSON

- [ ] **Step 1: Creare la struttura di directory**

```bash
cd ~/Desktop/SwissBunker
mkdir -p spikes/phase-0/{lib,tools,results}
touch spikes/phase-0/results/.gitkeep
```

- [ ] **Step 2: Scrivere `spikes/phase-0/lib/probe.js`**

```javascript
// Shared harness for Phase 0 probes.
// Deliberately a classic script with no imports and no dependencies: the whole point of
// this spike is to find out what survives a null origin, so the harness itself must not
// rely on anything we are still trying to prove works.
(function (global) {
  'use strict';

  var state = null;

  function detectContext() {
    return {
      origin: global.location.origin,       // "null" under file://
      protocol: global.location.protocol,
      userAgent: navigator.userAgent,
      platform: navigator.userAgentData ? navigator.userAgentData.platform : navigator.platform,
      isSecureContext: global.isSecureContext,
      crossOriginIsolated: global.crossOriginIsolated,
      hasSharedArrayBuffer: typeof SharedArrayBuffer !== 'undefined',
      hasWebGPU: typeof navigator.gpu !== 'undefined',
      deviceMemoryGB: navigator.deviceMemory || null,
      hardwareConcurrency: navigator.hardwareConcurrency || null
    };
  }

  function render() {
    var el = document.getElementById('probe-output');
    if (!el) { return; }
    el.textContent = JSON.stringify(state, null, 2);
  }

  var Probe = {
    init: function (id, title) {
      state = {
        probe: id,
        title: title,
        // Timestamp is filled in by the operator when filing the result, not here:
        // the clock of a random test machine is not trustworthy metadata.
        context: detectContext(),
        checks: {},
        measurements: {},
        info: {}
      };
      document.title = 'Probe ' + id + ' — ' + title;
      render();
      return state;
    },

    check: async function (name, fn) {
      var result;
      try {
        var value = await fn();
        result = { ok: value !== false && value !== null && value !== undefined, detail: value };
      } catch (err) {
        // A thrown exception IS the finding here, so it is recorded rather than propagated.
        result = { ok: false, detail: String(err && err.message ? err.message : err) };
      }
      state.checks[name] = result;
      render();
      return result;
    },

    measure: async function (name, fn, runs) {
      runs = runs || 20;
      var samples = [];
      for (var i = 0; i < runs; i++) {
        var t0 = performance.now();
        await fn(i);
        samples.push(performance.now() - t0);
      }
      samples.sort(function (a, b) { return a - b; });
      var pick = function (q) {
        return samples[Math.min(samples.length - 1, Math.floor(samples.length * q))];
      };
      var result = {
        runs: runs,
        p50: Math.round(pick(0.50) * 1000) / 1000,
        p95: Math.round(pick(0.95) * 1000) / 1000,
        min: Math.round(samples[0] * 1000) / 1000,
        max: Math.round(samples[samples.length - 1] * 1000) / 1000
      };
      state.measurements[name] = result;
      render();
      return result;
    },

    info: function (key, value) {
      state.info[key] = value;
      render();
    },

    finish: function () {
      var blob = new Blob([JSON.stringify(state, null, 2)], { type: 'application/json' });
      var a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = state.probe + '-' + (state.context.protocol === 'file:' ? 'file' : 'http') + '.json';
      a.textContent = 'Download result JSON';
      a.style.cssText = 'display:inline-block;margin:1rem 0;padding:.6rem 1rem;background:#111;color:#fff;text-decoration:none;border-radius:6px';
      document.body.appendChild(a);
      render();
      return state;
    }
  };

  global.Probe = Probe;
}(window));
```

- [ ] **Step 3: Scrivere `spikes/phase-0/tools/make-fixtures.sh`**

```bash
#!/usr/bin/env bash
# Generates the test fixtures for Phase 0.
#
# The exFAT disk image is the key trick: it reproduces exFAT's *semantics* (no permissions,
# no symlinks, its 32-bit heritage, allocation behaviour) without needing physical hardware.
# It does NOT reproduce USB *latency* — the real disk is still required for P3's timing
# numbers. Semantics first, timing later.
set -euo pipefail

FIXTURES="${1:-$HOME/swissbunker-fixtures}"
mkdir -p "$FIXTURES"

echo "==> Creating 120 GB sparse exFAT image (uses only the space actually written)"
if [ ! -f "$FIXTURES/exfat-test.sparseimage" ]; then
  hdiutil create -size 120g -fs exFAT -volname SWISSTEST -type SPARSE \
    -layout GPTSPUD "$FIXTURES/exfat-test" >/dev/null
fi

echo "==> Mounting"
hdiutil attach "$FIXTURES/exfat-test.sparseimage" >/dev/null
MOUNT="/Volumes/SWISSTEST"

echo "==> Generating a 12 GB file with verifiable content"
# Every 4096-byte page begins with its own page number in ASCII, so a range read can be
# checked for correctness and not merely for "it returned some bytes".
python3 - "$MOUNT/large-verifiable.bin" <<'PY'
import sys
path = sys.argv[1]
PAGE = 4096
PAGES = 3 * 1024 * 1024          # 12 GB
with open(path, 'wb') as f:
    for i in range(PAGES):
        header = f"PAGE:{i:012d}:".encode('ascii')
        f.write(header + bytes(PAGE - len(header)))
        if i % 262144 == 0:
            print(f"  {i * PAGE / 1e9:.1f} GB", flush=True)
PY

echo "==> Generating a 10 GB SQLite database with an FTS5 index"
python3 - "$MOUNT/fts-test.sqlite" <<'PY'
import sqlite3, sys, random, string
path = sys.argv[1]
con = sqlite3.connect(path)
con.execute("PRAGMA journal_mode=OFF")
con.execute("PRAGMA synchronous=OFF")
con.execute("PRAGMA page_size=4096")
con.execute("CREATE VIRTUAL TABLE docs USING fts5(title, body)")
words = [''.join(random.choices(string.ascii_lowercase, k=random.randint(3, 11)))
         for _ in range(20000)]
# A known needle planted at a known row lets the probe assert on an exact result
# instead of on "some rows came back".
BATCH, TOTAL = 5000, 2_000_000
rows = []
for i in range(TOTAL):
    body = ' '.join(random.choices(words, k=180))
    if i == 1_337_000:
        body += ' xyzzyneedlemarker'
    rows.append((f'Document {i}', body))
    if len(rows) >= BATCH:
        con.executemany("INSERT INTO docs(title, body) VALUES (?, ?)", rows)
        rows.clear()
        if i % 250000 == 0:
            con.commit(); print(f"  {i:,} rows", flush=True)
if rows:
    con.executemany("INSERT INTO docs(title, body) VALUES (?, ?)", rows)
con.commit()
con.execute("INSERT INTO docs(docs) VALUES('optimize')")
con.commit()
con.close()
PY

echo "==> Fetching a small real ZIM for format realism"
curl -fL --retry 3 -o "$MOUNT/wikimed.zim" \
  "https://download.kiwix.org/zim/wikipedia/wikipedia_en_medicine_nopic.zim" \
  || echo "    WARNING: ZIM download failed — the other probes can still run"

echo
echo "Fixtures ready at $MOUNT"
ls -lh "$MOUNT"
echo
echo "Unmount with: hdiutil detach $MOUNT"
```

- [ ] **Step 4: Scrivere `spikes/phase-0/tools/serve.sh`**

```bash
#!/usr/bin/env bash
# The http:// control condition. Every probe must be run twice — here and from file:// —
# because a probe that fails in both places has a bug, while a probe that fails only under
# file:// has found what we are looking for.
set -euo pipefail
cd "$(dirname "$0")/.."
echo "Control baseline: http://localhost:8000"
echo "Run the same probe from file:// and compare the two JSON records."
python3 -m http.server 8000
```

- [ ] **Step 5: Rendere eseguibili gli script e verificarne la sintassi**

```bash
cd ~/Desktop/SwissBunker
chmod +x spikes/phase-0/tools/*.sh
bash -n spikes/phase-0/tools/make-fixtures.sh && echo "make-fixtures.sh: syntax OK"
bash -n spikes/phase-0/tools/serve.sh && echo "serve.sh: syntax OK"
node --check spikes/phase-0/lib/probe.js && echo "probe.js: syntax OK"
```

Atteso: tre righe `OK`.

- [ ] **Step 6: Scrivere `spikes/phase-0/README.md`**

````markdown
# Phase 0 — Feasibility spike

This directory answers one question: **can the Portable runtime exist?**

Everything here is throwaway. No production code may import from it.

## Order of execution

1. `tools/make-fixtures.sh` — generate the exFAT image and test data (~30 min, one time)
2. `tools/serve.sh` — start the http:// control baseline in a second terminal
3. Run probes P1 to P6 in order. Each one twice: from `file://` and from `http://localhost:8000`
4. Save every downloaded JSON into `results/`
5. Open `report.html` to build the matrix
6. Write the findings document

## Probes

| Probe | Question | Blocks what if it fails |
|-------|----------|-------------------------|
| P1 | What forms of code loading survive a null origin? | Everything — run first |
| P2 | Can a `file://` page enumerate a directory and get `File` objects? | The whole Portable mode (R1) |
| P3 | Is `File.slice()` correct and fast on a multi-GB exFAT file? | The index format (R6) |
| P4 | Can SQLite query a 10 GB database through range reads? | The search engine |
| P5 | Are WebGPU and Web Workers available under a null origin? | Local inference |
| P6 | Can LLM weights load from `File` objects instead of HTTP? | The chat feature (R2) |

## Naming of result files

`results/<probe>-<protocol>-<browser>-<os>.json`, e.g. `p3-file-chrome-macos.json`.
The harness names the first two segments; the operator appends browser and OS.

## Rule

A probe that fails under **both** `file://` and `http://` has a bug in the probe.
Fix the probe. Only a `file://`-specific failure is a finding.
````

- [ ] **Step 7: Commit**

```bash
cd ~/Desktop/SwissBunker
git add spikes/phase-0
git commit -m "spike(phase-0): probe harness, fixtures generator, control baseline"
```

---

### Task 1: P1 — Caricamento del codice sotto origin nullo

**Files:**
- Create: `spikes/phase-0/p1-module-loading.html`
- Create: `spikes/phase-0/lib/vendor-probe.js`
- Create: `spikes/phase-0/tools/build-vendor.sh`

**Interfaces:**
- Consumes: `window.Probe` (Task 0)
- Produces: il verdetto sulla forma di packaging che tutti i probe successivi devono usare.
  Se i moduli ES falliscono, P2–P6 usano bundle IIFE e `tools/build-vendor.sh` diventa
  obbligatorio.

**Perché questo probe è il primo:** ogni altro probe dipende dal riuscire a *caricare* il
proprio codice. Testare WebGPU con uno script che non parte produce un falso negativo che
costerebbe giorni.

- [ ] **Step 1: Scrivere `spikes/phase-0/lib/vendor-probe.js`**

```javascript
// Dual-format probe target: the classic path sets a global, the module path is imported
// dynamically. Loading this file two different ways is what P1 measures.
window.__VENDOR_CLASSIC_LOADED__ = true;
export const marker = 'esm-loaded';
```

- [ ] **Step 2: Scrivere `spikes/phase-0/p1-module-loading.html`**

```html
<!doctype html>
<meta charset="utf-8">
<title>P1</title>
<style>body{font:14px/1.6 ui-monospace,monospace;margin:2rem;max-width:80ch}pre{background:#f4f4f5;padding:1rem;border-radius:8px;overflow-x:auto}</style>
<h1>P1 — Code loading under a null origin</h1>
<p>Run from <code>file://</code> and from <code>http://localhost:8000</code>, then compare.</p>
<pre id="probe-output"></pre>
<script src="lib/probe.js"></script>
<script>
(async function () {
  Probe.init('p1', 'Code loading under a null origin');

  // 1. Classic <script src>. If this fails, nothing else is possible — probe.js itself
  //    was loaded this way, so its presence is the evidence.
  await Probe.check('classic_script_tag', function () {
    return typeof Probe === 'object';
  });

  // 2. Dynamic import of an ES module — the most likely casualty of a null origin,
  //    because module fetches are always CORS-checked.
  await Probe.check('dynamic_esm_import', async function () {
    var mod = await import('./lib/vendor-probe.js');
    return mod.marker === 'esm-loaded';
  });

  // 3. fetch() of a sibling file — needed by any library that loads its own .wasm.
  await Probe.check('fetch_sibling_file', async function () {
    var res = await fetch('lib/probe.js');
    var text = await res.text();
    return text.length > 100;
  });

  // 4. WebAssembly from inline bytes — the fallback when fetch is blocked. These eight
  //    bytes are the smallest valid wasm module: magic number plus version.
  await Probe.check('wasm_instantiate_inline', async function () {
    var bytes = new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
    var mod = await WebAssembly.instantiate(bytes);
    return typeof mod.instance === 'object';
  });

  // 5. Blob URL script injection — the standard escape hatch when module loading is blocked.
  await Probe.check('blob_url_script', function () {
    return new Promise(function (resolve) {
      var blob = new Blob(['window.__BLOB_SCRIPT_RAN__ = true;'], { type: 'text/javascript' });
      var s = document.createElement('script');
      s.src = URL.createObjectURL(blob);
      s.onload = function () { resolve(window.__BLOB_SCRIPT_RAN__ === true); };
      s.onerror = function () { resolve(false); };
      document.head.appendChild(s);
    });
  });

  // 6. Storage under a null origin. localStorage and IndexedDB are known to be unreliable
  //    here, and the answer decides where user settings are allowed to live.
  await Probe.check('localStorage_usable', function () {
    localStorage.setItem('__probe__', '1');
    var ok = localStorage.getItem('__probe__') === '1';
    localStorage.removeItem('__probe__');
    return ok;
  });

  await Probe.check('indexeddb_usable', function () {
    return new Promise(function (resolve) {
      var req;
      try { req = indexedDB.open('__probe__', 1); }
      catch (e) { resolve(false); return; }
      req.onsuccess = function () { req.result.close(); resolve(true); };
      req.onerror = function () { resolve(false); };
      req.onblocked = function () { resolve(false); };
      setTimeout(function () { resolve(false); }, 3000);
    });
  });

  // 7. OPFS — the sandboxed writable store, our only option for user data in Portable mode.
  await Probe.check('opfs_usable', async function () {
    if (!navigator.storage || !navigator.storage.getDirectory) { return false; }
    var root = await navigator.storage.getDirectory();
    var fh = await root.getFileHandle('__probe__', { create: true });
    var w = await fh.createWritable();
    await w.write('ok');
    await w.close();
    var f = await fh.getFile();
    var text = await f.text();
    await root.removeEntry('__probe__');
    return text === 'ok';
  });

  Probe.finish();
}());
</script>
```

- [ ] **Step 3: Eseguire nella condizione di controllo**

```bash
cd ~/Desktop/SwissBunker/spikes/phase-0
./tools/serve.sh &
open -a "Google Chrome" http://localhost:8000/p1-module-loading.html
```

Atteso: **tutti e otto i check verdi**. Se qualcuno fallisce qui, il probe ha un bug — si
corregge prima di procedere. Scaricare il JSON in `results/p1-http-chrome-macos.json`.

- [ ] **Step 4: Eseguire nella condizione reale**

```bash
open -a "Google Chrome" ~/Desktop/SwissBunker/spikes/phase-0/p1-module-loading.html
```

Atteso sulla base della documentazione delle piattaforme — **da confermare, non da dare per
scontato**: `classic_script_tag`, `wasm_instantiate_inline` e `blob_url_script` passano;
`dynamic_esm_import` e `fetch_sibling_file` falliscono per CORS su origin nullo;
`indexeddb_usable` incerto. Scaricare in `results/p1-file-chrome-macos.json`.

- [ ] **Step 5: Ripetere su Edge, Safari 26 e Firefox 147, e su Windows**

Minimo otto esecuzioni: 4 browser × 2 protocolli su macOS, poi Chrome/Edge/Firefox su Windows.

- [ ] **Step 6: Scrivere `spikes/phase-0/tools/build-vendor.sh`**

Da eseguire **solo se** `dynamic_esm_import` o `fetch_sibling_file` falliscono da `file://`,
perché in quel caso ogni libreria di terze parti va pre-bundlata in forma IIFE con il wasm
incorporato.

```bash
#!/usr/bin/env bash
# Pre-bundles third-party libraries into classic IIFE scripts with inlined wasm, for the
# case where file:// blocks module loading and fetch.
#
# The output is committed on purpose: a probe must run by opening a file, with no build
# step in between, because "needs a build step" is itself one of the failure modes being
# measured.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v npx >/dev/null || { echo "npx required"; exit 1; }

mkdir -p vendor
npx --yes esbuild@0.23.0 \
  --bundle \
  --format=iife \
  --global-name=SqlJs \
  --loader:.wasm=base64 \
  --outfile=vendor/sql-wasm.iife.js \
  node_modules/sql.js/dist/sql-wasm.js

echo "Built vendor/sql-wasm.iife.js ($(du -h vendor/sql-wasm.iife.js | cut -f1))"
```

- [ ] **Step 7: Registrare il verdetto e committare**

Aggiungere a `spikes/phase-0/README.md` una sezione `## P1 verdict` che dichiara la forma di
packaging imposta dai risultati.

```bash
cd ~/Desktop/SwissBunker
chmod +x spikes/phase-0/tools/build-vendor.sh
git add spikes/phase-0
git commit -m "spike(phase-0): P1 code loading under null origin"
```

---

### Task 2: P2 — Enumerazione directory e accesso ai file

**Files:**
- Create: `spikes/phase-0/p2-directory-picker.html`

**Interfaces:**
- Consumes: `window.Probe` (Task 0); la forma di packaging decisa da P1 (Task 1)
- Produces: il verdetto su **rischio R1**. Se nessun metodo di selezione directory funziona
  da `file://` su nessun browser, la modalità Portable nella forma progettata è morta e si
  attiva il piano B della spec §14.

**Il cuore della questione:** la spec scarta la File System Access API perché è solo
Chromium, e punta su `<input type="file" webkitdirectory>` come alternativa universale. Qui
si verifica se quella scommessa regge, e soprattutto quanto costa enumerare una directory
che contiene file da decine di gigabyte.

- [ ] **Step 1: Scrivere `spikes/phase-0/p2-directory-picker.html`**

```html
<!doctype html>
<meta charset="utf-8">
<title>P2</title>
<style>body{font:14px/1.6 ui-monospace,monospace;margin:2rem;max-width:80ch}pre{background:#f4f4f5;padding:1rem;border-radius:8px;overflow-x:auto}button,input{font:inherit;margin:.5rem 0}</style>
<h1>P2 — Directory enumeration under a null origin</h1>
<p>Point every control below at the mounted <code>SWISSTEST</code> volume.</p>

<h3>Method A — webkitdirectory (expected to be universal)</h3>
<input type="file" id="dir-input" webkitdirectory multiple>

<h3>Method B — showDirectoryPicker (Chromium only)</h3>
<button id="fsa-btn">Pick directory via File System Access</button>

<h3>Method C — drag and drop a folder</h3>
<div id="drop" style="border:2px dashed #999;padding:2rem;text-align:center;border-radius:8px">Drop the volume here</div>

<pre id="probe-output"></pre>
<script src="lib/probe.js"></script>
<script>
(function () {
  Probe.init('p2', 'Directory enumeration under a null origin');

  // API availability is recorded before any user interaction, because "the API does not
  // exist" and "the API exists but the call is rejected" are different findings.
  Probe.check('fsa_api_present', function () {
    return typeof window.showDirectoryPicker === 'function';
  });
  Probe.check('webkitdirectory_supported', function () {
    return 'webkitdirectory' in document.createElement('input');
  });
  Probe.check('datatransfer_items_present', function () {
    return typeof DataTransferItem !== 'undefined'
      && ('webkitGetAsEntry' in DataTransferItem.prototype);
  });

  function summarise(files) {
    var total = 0, biggest = null;
    for (var i = 0; i < files.length; i++) {
      total += files[i].size;
      if (!biggest || files[i].size > biggest.size) { biggest = files[i]; }
    }
    return {
      count: files.length,
      totalBytes: total,
      totalGB: Math.round(total / 1e9 * 100) / 100,
      largestFile: biggest ? { name: biggest.name, bytes: biggest.size,
                               GB: Math.round(biggest.size / 1e9 * 100) / 100 } : null,
      // A File over 4 GB reported with a wrong size is exactly the R6 failure mode.
      largestOver4GB: biggest ? biggest.size > 4 * 1024 * 1024 * 1024 : false
    };
  }

  // --- Method A: webkitdirectory -------------------------------------------------------
  document.getElementById('dir-input').addEventListener('change', async function (e) {
    var t0 = performance.now();
    var files = Array.prototype.slice.call(e.target.files);
    var elapsed = performance.now() - t0;

    await Probe.check('webkitdirectory_returned_files', function () { return files.length > 0; });
    Probe.info('webkitdirectory_enumeration_ms', Math.round(elapsed));
    Probe.info('webkitdirectory_summary', summarise(files));

    // Reading one range proves the handle is live, not merely metadata.
    await Probe.check('webkitdirectory_file_readable', async function () {
      var target = files.filter(function (f) { return f.size > 1e9; })[0] || files[0];
      var buf = await target.slice(0, 16).arrayBuffer();
      return buf.byteLength === 16;
    });

    // Size correctness past 4 GB: read the final 16 bytes and confirm no error.
    await Probe.check('large_file_tail_readable', async function () {
      var big = files.filter(function (f) { return f.size > 4 * 1024 * 1024 * 1024; })[0];
      if (!big) { return 'no file over 4 GB in this directory — inconclusive'; }
      var buf = await big.slice(big.size - 16, big.size).arrayBuffer();
      return buf.byteLength === 16;
    });

    Probe.finish();
  });

  // --- Method B: File System Access ----------------------------------------------------
  document.getElementById('fsa-btn').addEventListener('click', async function () {
    await Probe.check('fsa_picker_callable', async function () {
      if (typeof window.showDirectoryPicker !== 'function') { return false; }
      var handle = await window.showDirectoryPicker();
      var names = [];
      for await (var entry of handle.values()) { names.push(entry.name); }
      return { entries: names.length, sample: names.slice(0, 5) };
    });
  });

  // --- Method C: drag and drop ---------------------------------------------------------
  var drop = document.getElementById('drop');
  drop.addEventListener('dragover', function (e) { e.preventDefault(); });
  drop.addEventListener('drop', async function (e) {
    e.preventDefault();
    await Probe.check('drop_folder_readable', async function () {
      var items = Array.prototype.slice.call(e.dataTransfer.items);
      var entry = items[0] && items[0].webkitGetAsEntry && items[0].webkitGetAsEntry();
      if (!entry) { return false; }
      if (!entry.isDirectory) { return 'dropped item is not a directory'; }
      return await new Promise(function (resolve) {
        entry.createReader().readEntries(function (entries) {
          resolve({ entries: entries.length,
                    sample: entries.slice(0, 5).map(function (x) { return x.name; }) });
        }, function () { resolve(false); });
      });
    });
  });
}());
</script>
```

- [ ] **Step 2: Montare le fixture ed eseguire il controllo**

```bash
cd ~/Desktop/SwissBunker/spikes/phase-0
hdiutil attach ~/swissbunker-fixtures/exfat-test.sparseimage
./tools/serve.sh &
open -a "Google Chrome" http://localhost:8000/p2-directory-picker.html
```

Selezionare `/Volumes/SWISSTEST` con il Metodo A. Atteso: `webkitdirectory_returned_files`
verde, file più grande intorno ai 12 GB, `large_file_tail_readable` verde. Salvare come
`results/p2-http-chrome-macos.json`.

- [ ] **Step 3: Eseguire nella condizione reale**

```bash
open -a "Google Chrome" ~/Desktop/SwissBunker/spikes/phase-0/p2-directory-picker.html
```

Salvare come `results/p2-file-chrome-macos.json`.

- [ ] **Step 4: Ripetere su Safari 26, Firefox 147, Edge, e su Windows**

Su Safari e Firefox `fsa_api_present` deve risultare falso: è la conferma che la scelta
architetturale di non dipendere da quell'API era necessaria. Ciò che conta è che il
**Metodo A passi ovunque**.

- [ ] **Step 5: Registrare il verdetto R1 e committare**

Aggiungere a `spikes/phase-0/README.md` una sezione `## P2 verdict — risk R1` che dichiara,
per ciascun browser, quale metodo di selezione funziona da `file://`.

```bash
cd ~/Desktop/SwissBunker
git add spikes/phase-0
git commit -m "spike(phase-0): P2 directory enumeration, risk R1 evidence"
```

---

### Task 3: P3 — Correttezza e latenza di `File.slice()`

**Files:**
- Create: `spikes/phase-0/p3-range-read.html`

**Interfaces:**
- Consumes: `window.Probe` (Task 0); il metodo di selezione validato da P2 (Task 2)
- Produces: il **budget di latenza per lettura a range**, il numero che decide se i parametri
  IVF della spec §6.3 (`nprobe` = 32, circa 5.6 MB letti per query) rispettano l'obiettivo
  NF2 di 800 ms.

**Perché la correttezza va misurata e non assunta:** le fixture scrivono il numero di pagina
in testa a ogni pagina da 4096 byte, quindi una lettura a offset arbitrario può essere
verificata contro il valore atteso. Un `slice()` che restituisce byte *sbagliati* oltre una
certa soglia è un fallimento silenzioso, molto peggiore di un errore.

- [ ] **Step 1: Scrivere `spikes/phase-0/p3-range-read.html`**

```html
<!doctype html>
<meta charset="utf-8">
<title>P3</title>
<style>body{font:14px/1.6 ui-monospace,monospace;margin:2rem;max-width:80ch}pre{background:#f4f4f5;padding:1rem;border-radius:8px;overflow-x:auto}</style>
<h1>P3 — Range-read correctness and latency</h1>
<p>Select the <code>SWISSTEST</code> volume. The probe looks for <code>large-verifiable.bin</code>.</p>
<input type="file" id="dir-input" webkitdirectory multiple>
<pre id="probe-output"></pre>
<script src="lib/probe.js"></script>
<script>
(function () {
  Probe.init('p3', 'Range-read correctness and latency');

  var PAGE = 4096;
  var decoder = new TextDecoder();

  // The fixture writes "PAGE:%012d:" at the start of every 4096-byte page, so the expected
  // content of any offset is computable. This turns "did it read?" into "did it read the
  // RIGHT bytes?" — the distinction that matters for a silent-corruption failure mode.
  function expectedHeader(pageIndex) {
    return 'PAGE:' + String(pageIndex).padStart(12, '0') + ':';
  }

  async function readPage(file, pageIndex) {
    var off = pageIndex * PAGE;
    var buf = await file.slice(off, off + 32).arrayBuffer();
    return decoder.decode(new Uint8Array(buf));
  }

  document.getElementById('dir-input').addEventListener('change', async function (e) {
    var files = Array.prototype.slice.call(e.target.files);
    var file = files.filter(function (f) { return f.name === 'large-verifiable.bin'; })[0];

    if (!file) {
      await Probe.check('fixture_found', function () { return false; });
      Probe.finish();
      return;
    }

    await Probe.check('fixture_found', function () { return true; });
    Probe.info('file', { name: file.name, bytes: file.size,
                         GB: Math.round(file.size / 1e9 * 100) / 100 });
    var totalPages = Math.floor(file.size / PAGE);

    // 1. Correctness at the boundaries and at the 4 GB line, where 32-bit offset bugs live.
    await Probe.check('correctness_at_key_offsets', async function () {
      var fourGBPage = Math.floor(4 * 1024 * 1024 * 1024 / PAGE);
      var targets = [0, 1, 1000, fourGBPage - 1, fourGBPage, fourGBPage + 1,
                     Math.floor(totalPages / 2), totalPages - 1];
      var failures = [];
      for (var i = 0; i < targets.length; i++) {
        var p = targets[i];
        if (p < 0 || p >= totalPages) { continue; }
        var got = await readPage(file, p);
        var want = expectedHeader(p);
        if (got.indexOf(want) !== 0) {
          failures.push({ page: p, expected: want, got: got.slice(0, 20) });
        }
      }
      return failures.length === 0 ? true : failures;
    });

    // 2. Correctness under random access across the whole file.
    await Probe.check('correctness_random_200', async function () {
      var failures = 0;
      for (var i = 0; i < 200; i++) {
        var p = Math.floor(Math.random() * totalPages);
        var got = await readPage(file, p);
        if (got.indexOf(expectedHeader(p)) !== 0) { failures++; }
      }
      return failures === 0 ? true : failures + ' of 200 pages returned wrong bytes';
    });

    // 3. Latency of a single small random read — the HNSW-style access pattern the spec
    //    rejected. This measurement is what justifies choosing IVF.
    await Probe.measure('random_4kb_read_ms', async function () {
      var p = Math.floor(Math.random() * totalPages);
      await file.slice(p * PAGE, p * PAGE + PAGE).arrayBuffer();
    }, 200);

    // 4. Latency of one large contiguous read — the IVF access pattern.
    await Probe.measure('contiguous_8mb_read_ms', async function () {
      var maxOff = Math.max(0, file.size - 8 * 1024 * 1024);
      var off = Math.floor(Math.random() * maxOff);
      await file.slice(off, off + 8 * 1024 * 1024).arrayBuffer();
    }, 20);

    // 5. The decisive comparison: 1400 scattered small reads (an HNSW traversal) against
    //    one 5.6 MB contiguous read (an IVF nprobe=32 scan). Similar byte counts,
    //    opposite access patterns.
    await Probe.measure('scattered_1400x4kb_ms', async function () {
      for (var i = 0; i < 1400; i++) {
        var p = Math.floor(Math.random() * totalPages);
        await file.slice(p * PAGE, p * PAGE + PAGE).arrayBuffer();
      }
    }, 5);

    await Probe.measure('contiguous_5_6mb_ms', async function () {
      var bytes = Math.floor(5.6 * 1024 * 1024);
      var off = Math.floor(Math.random() * Math.max(0, file.size - bytes));
      await file.slice(off, off + bytes).arrayBuffer();
    }, 20);

    // 6. Parallel versus sequential reads — decides whether the IVF reader should issue
    //    its list reads concurrently.
    await Probe.measure('parallel_32x256kb_ms', async function () {
      var jobs = [];
      for (var i = 0; i < 32; i++) {
        var off = Math.floor(Math.random() * Math.max(0, file.size - 262144));
        jobs.push(file.slice(off, off + 262144).arrayBuffer());
      }
      await Promise.all(jobs);
    }, 20);

    await Probe.measure('sequential_32x256kb_ms', async function () {
      for (var i = 0; i < 32; i++) {
        var off = Math.floor(Math.random() * Math.max(0, file.size - 262144));
        await file.slice(off, off + 262144).arrayBuffer();
      }
    }, 20);

    Probe.finish();
  });
}());
</script>
```

- [ ] **Step 2: Eseguire sull'immagine exFAT, controllo e condizione reale**

```bash
cd ~/Desktop/SwissBunker/spikes/phase-0
hdiutil attach ~/swissbunker-fixtures/exfat-test.sparseimage
./tools/serve.sh &
open -a "Google Chrome" http://localhost:8000/p3-range-read.html
open -a "Google Chrome" ~/Desktop/SwissBunker/spikes/phase-0/p3-range-read.html
```

Salvare come `results/p3-http-chrome-macos-image.json` e `results/p3-file-chrome-macos-image.json`.
Il suffisso `-image` distingue queste misure, che riflettono la semantica exFAT ma **non** la
latenza USB.

- [ ] **Step 3: Ripetere su un disco esterno reale**

Copiare `large-verifiable.bin` su un disco esterno formattato exFAT ed eseguire di nuovo,
salvando con suffisso `-usb`. **Sono queste le misure che contano** per il budget di latenza.

- [ ] **Step 4: Verificare i criteri di accettazione**

| Misura | Soglia | Conseguenza se fallisce |
|---|---|---|
| `correctness_at_key_offsets` | deve passare | Bug di offset a 32 bit: il formato indice va ripensato o i file frammentati sotto i 4 GB (R6) |
| `correctness_random_200` | deve passare | Corruzione silenziosa: modalità Portable inutilizzabile |
| `contiguous_5_6mb_ms` p95 | < 200 ms | I parametri IVF di §6.3 vanno ridotti (`nprobe` più basso) |
| `scattered_1400x4kb_ms` p50 | qualsiasi valore | Il confronto con la riga sopra è la prova a supporto della scelta IVF |
| `parallel_32x256kb_ms` vs `sequential_32x256kb_ms` | — | Se il parallelo vince nettamente, il reader IVF va scritto concorrente |

- [ ] **Step 5: Ripetere su Safari, Firefox, Edge e Windows, poi committare**

```bash
cd ~/Desktop/SwissBunker
git add spikes/phase-0
git commit -m "spike(phase-0): P3 range-read correctness and latency budget"
```

---

### Task 4: P4 — SQLite su database grande letto a range

**Files:**
- Create: `spikes/phase-0/lib/file-vfs.js`
- Create: `spikes/phase-0/p4-sqlite-lazy.html`
- Modify: `.gitignore` (escludere `spikes/phase-0/node_modules/`)

**Interfaces:**
- Consumes: `window.Probe` (Task 0); i risultati di latenza di P3 (Task 3)
- Produces: `window.LazyFileReader`, con:
  - `new LazyFileReader(file: File, opts?: {chunkSize?: number})`
  - `.prefetch(offset: number, length: number): Promise<void>` — assicura che i chunk coprenti quell'intervallo siano in cache
  - `.readSync(offset: number, length: number): Uint8Array | null` — lettura sincrona da cache, `null` in caso di miss
  - `.stats: {hits, misses, bytesRead, sliceCalls}`

  Più la prova che il motore di ricerca della spec §6.4 è realizzabile: una query FTS5 su un
  database da 10 GB mai caricato interamente in memoria.

**Il vincolo che rende il probe non banale:** `sql.js` carica di default l'intero database in
memoria, il che con 10 GB è impossibile. Serve un backend di lettura pigra che traduca le
richieste di pagina di SQLite in chiamate `File.slice()`. È il pattern di `sql.js-httpvfs`,
riscritto per una sorgente locale invece che HTTP.

- [ ] **Step 1: Scrivere `spikes/phase-0/lib/file-vfs.js`**

```javascript
// Lazy page reader that bridges SQLite's page requests to File.slice().
//
// Emscripten's createLazyFile expects a synchronous byte-range getter, but File.slice() is
// asynchronous. The bridge is a read-ahead cache: prefetch generously around every requested
// offset so most subsequent synchronous requests are already satisfied, and surface a miss
// loudly instead of silently returning zeroes — silent zeroes would look like a corrupt
// database and cost days to diagnose.
(function (global) {
  'use strict';

  function LazyFileReader(file, opts) {
    opts = opts || {};
    this.file = file;
    this.chunkSize = opts.chunkSize || 1024 * 1024;   // 1 MB read-ahead window
    this.cache = new Map();
    this.stats = { hits: 0, misses: 0, bytesRead: 0, sliceCalls: 0 };
  }

  LazyFileReader.prototype.chunkIndex = function (offset) {
    return Math.floor(offset / this.chunkSize);
  };

  // Asynchronously ensure the chunks covering [offset, offset+length) are cached.
  LazyFileReader.prototype.prefetch = async function (offset, length) {
    var first = this.chunkIndex(offset);
    var last = this.chunkIndex(offset + length - 1);
    var jobs = [];
    for (var i = first; i <= last; i++) {
      if (this.cache.has(i)) { continue; }
      jobs.push(this._loadChunk(i));
    }
    await Promise.all(jobs);
  };

  LazyFileReader.prototype._loadChunk = async function (index) {
    var start = index * this.chunkSize;
    var end = Math.min(this.file.size, start + this.chunkSize);
    this.stats.sliceCalls++;
    var buf = await this.file.slice(start, end).arrayBuffer();
    this.stats.bytesRead += buf.byteLength;
    this.cache.set(index, new Uint8Array(buf));
  };

  // Synchronous read, satisfied from cache only. Returns null on a miss so the caller can
  // decide what to do — never zeroes.
  LazyFileReader.prototype.readSync = function (offset, length) {
    var out = new Uint8Array(length);
    var written = 0;
    while (written < length) {
      var pos = offset + written;
      var idx = this.chunkIndex(pos);
      var chunk = this.cache.get(idx);
      if (!chunk) { this.stats.misses++; return null; }
      var inChunk = pos - idx * this.chunkSize;
      var take = Math.min(length - written, chunk.length - inChunk);
      if (take <= 0) { this.stats.misses++; return null; }
      out.set(chunk.subarray(inChunk, inChunk + take), written);
      written += take;
    }
    this.stats.hits++;
    return out;
  };

  global.LazyFileReader = LazyFileReader;
}(window));
```

- [ ] **Step 2: Scrivere `spikes/phase-0/p4-sqlite-lazy.html`**

```html
<!doctype html>
<meta charset="utf-8">
<title>P4</title>
<style>body{font:14px/1.6 ui-monospace,monospace;margin:2rem;max-width:80ch}pre{background:#f4f4f5;padding:1rem;border-radius:8px;overflow-x:auto}</style>
<h1>P4 — SQLite over range reads</h1>
<p>Select the <code>SWISSTEST</code> volume. The probe looks for <code>fts-test.sqlite</code>.</p>
<input type="file" id="dir-input" webkitdirectory multiple>
<pre id="probe-output"></pre>
<script src="lib/probe.js"></script>
<script src="lib/file-vfs.js"></script>
<script>
(function () {
  Probe.init('p4', 'SQLite over range reads');

  document.getElementById('dir-input').addEventListener('change', async function (e) {
    var files = Array.prototype.slice.call(e.target.files);
    var db = files.filter(function (f) { return f.name === 'fts-test.sqlite'; })[0];

    if (!db) {
      await Probe.check('fixture_found', function () { return false; });
      Probe.finish();
      return;
    }

    await Probe.check('fixture_found', function () { return true; });
    Probe.info('database', { bytes: db.size, GB: Math.round(db.size / 1e9 * 100) / 100 });

    var reader = new LazyFileReader(db, { chunkSize: 1024 * 1024 });

    // 1. The cheapest possible proof the reader works: the first 16 bytes of any SQLite
    //    file are the literal header string, terminated by a zero byte.
    await Probe.check('sqlite_header_valid', async function () {
      await reader.prefetch(0, 100);
      var head = reader.readSync(0, 15);
      if (!head) { return 'cache miss on the first page'; }
      return new TextDecoder().decode(head) === 'SQLite format 3';
    });

    // 2. Page size and page count come from the header, proving we can navigate the file's
    //    own structure rather than merely read arbitrary bytes.
    await Probe.check('header_fields_sane', async function () {
      await reader.prefetch(0, 100);
      var head = reader.readSync(0, 100);
      if (!head) { return 'cache miss'; }
      var dv = new DataView(head.buffer, head.byteOffset, head.byteLength);
      var pageSize = dv.getUint16(16);          // a stored value of 1 means 65536
      if (pageSize === 1) { pageSize = 65536; }
      var pageCount = dv.getUint32(28);
      Probe.info('sqlite_header', { pageSize: pageSize, pageCount: pageCount,
                                    impliedBytes: pageSize * pageCount });
      return pageSize === 4096 && pageCount > 0;
    });

    // 3. Read amplification: how many bytes must be pulled to answer one FTS5 query?
    //    This is the number that decides whether the design meets NF2, so it is measured
    //    rather than estimated. An FTS5 term lookup touches the term index B-tree (a few
    //    pages), then a doclist (contiguous), then content rows (scattered) — roughly 40
    //    page touches, simulated here over the real file layout.
    var bytesBefore = reader.stats.bytesRead;
    await Probe.measure('simulated_fts_query_ms', async function () {
      for (var i = 0; i < 40; i++) {
        var off = Math.floor(Math.random() * Math.max(0, db.size - 4096));
        await reader.prefetch(off, 4096);
        reader.readSync(off, 4096);
      }
    }, 20);

    Probe.info('reader_stats_after_simulation', JSON.parse(JSON.stringify(reader.stats)));
    // The delta across the measured runs only — reader.stats.bytesRead is cumulative from
    // page load and would otherwise fold the header prefetch into the per-query figure.
    Probe.info('bytes_per_simulated_query',
      Math.round((reader.stats.bytesRead - bytesBefore) / 20));

    // 4. Memory ceiling: the cache must never approach the heap limit. If a 10 GB database
    //    forces the cache past ~500 MB to stay useful, the chunk size is wrong.
    await Probe.check('cache_stayed_bounded', function () {
      var cachedBytes = reader.cache.size * reader.chunkSize;
      Probe.info('cache_bytes', cachedBytes);
      return cachedBytes < 500 * 1024 * 1024;
    });

    // 5. Heap headroom, to confirm constraint V4 (4 GB WASM32 heap) empirically.
    await Probe.check('heap_headroom', function () {
      if (!performance.memory) { return 'performance.memory unavailable in this browser'; }
      return {
        usedMB: Math.round(performance.memory.usedJSHeapSize / 1e6),
        limitMB: Math.round(performance.memory.jsHeapSizeLimit / 1e6)
      };
    });

    // 6. A real FTS5 query against the planted needle. The fixture put the token
    //    "xyzzyneedlemarker" in exactly one row out of two million, so the correct answer
    //    is known in advance: one row, titled "Document 1337000". A different answer means
    //    the lazy read path returns wrong bytes — the risk this probe exists to rule out.
    await Probe.check('real_fts5_query', async function () {
      if (typeof SqlJs === 'undefined') {
        return 'vendor bundle not built — run tools/build-vendor.sh, then reload';
      }
      // esbuild's --global-name may expose the init function directly or under .default,
      // depending on how sql.js's UMD wrapper is interpreted. Accept both.
      var initSqlJs = (typeof SqlJs === 'function') ? SqlJs : SqlJs.default;
      if (typeof initSqlJs !== 'function') { return 'SqlJs global is not callable'; }
      var SQL = await initSqlJs({ locateFile: function (f) { return f; } });
      var FS = SQL.FS;
      FS.createLazyFile('/', 'fts.sqlite', {
        length: db.size,
        read: function (offset, length) {
          var bytes = reader.readSync(offset, length);
          if (!bytes) { throw new Error('lazy read miss at offset ' + offset); }
          return bytes;
        }
      }, true, false);
      var handle = new SQL.Database('/fts.sqlite', { filename: true });
      var rows = handle.exec("SELECT title FROM docs WHERE docs MATCH 'xyzzyneedlemarker'");
      handle.close();
      if (!rows.length || !rows[0].values.length) { return 'needle not found'; }
      return { matched: rows[0].values.length, title: rows[0].values[0][0] };
    });

    Probe.finish();
  });
}());
</script>
```

- [ ] **Step 3: Eseguire i primi cinque check**

```bash
cd ~/Desktop/SwissBunker/spikes/phase-0
hdiutil attach ~/swissbunker-fixtures/exfat-test.sparseimage
./tools/serve.sh &
open -a "Google Chrome" http://localhost:8000/p4-sqlite-lazy.html
open -a "Google Chrome" ~/Desktop/SwissBunker/spikes/phase-0/p4-sqlite-lazy.html
```

Criteri: `sqlite_header_valid` e `header_fields_sane` devono passare;
`bytes_per_simulated_query` deve restare sotto i 50 MB; `cache_stayed_bounded` deve passare.
A questo punto `real_fts5_query` riporterà che il bundle non è stato costruito — è previsto.

- [ ] **Step 4: Costruire il bundle di `sql.js` ed eseguire la query vera**

Solo ora ha senso spendere tempo sul motore reale.

```bash
cd ~/Desktop/SwissBunker/spikes/phase-0
npm init -y >/dev/null 2>&1
npm install sql.js@1.11.0
./tools/build-vendor.sh
```

Aggiungere a `p4-sqlite-lazy.html`, subito dopo `<script src="lib/file-vfs.js"></script>`:

```html
<script src="vendor/sql-wasm.iife.js"></script>
```

Ricaricare la pagina in entrambi i protocolli. Atteso:
`{ matched: 1, title: "Document 1337000" }`.

- [ ] **Step 5: Committare**

```bash
cd ~/Desktop/SwissBunker
# .gitignore already carries a bare `node_modules/`, which git matches at any depth —
# no extra rule is needed for spikes/phase-0/node_modules/.
git add -A
git commit -m "spike(phase-0): P4 SQLite FTS5 over lazy range reads"
```

---

### Task 5: P5 — WebGPU e Web Worker sotto origin nullo

**Files:**
- Create: `spikes/phase-0/p5-webgpu-worker.html`

**Interfaces:**
- Consumes: `window.Probe` (Task 0); il verdetto di packaging di P1 (Task 1)
- Produces: la classificazione hardware che alimenta il tiering della spec §8.1, e la prova
  che il lavoro pesante può uscire dal thread principale.

- [ ] **Step 1: Scrivere `spikes/phase-0/p5-webgpu-worker.html`**

```html
<!doctype html>
<meta charset="utf-8">
<title>P5</title>
<style>body{font:14px/1.6 ui-monospace,monospace;margin:2rem;max-width:80ch}pre{background:#f4f4f5;padding:1rem;border-radius:8px;overflow-x:auto}</style>
<h1>P5 — WebGPU and Workers under a null origin</h1>
<pre id="probe-output"></pre>
<script src="lib/probe.js"></script>
<script>
(async function () {
  Probe.init('p5', 'WebGPU and Workers under a null origin');

  // 1. Is the API even exposed? Under file:// this depends on whether the browser treats
  //    a null origin as a secure context.
  await Probe.check('webgpu_api_present', function () {
    return typeof navigator.gpu !== 'undefined';
  });

  var device = null;

  // 2. Adapter and device acquisition. The adapter's limits directly determine which model
  //    tier from spec §8.1 this machine can run, so they are recorded in full.
  await Probe.check('webgpu_device_acquired', async function () {
    if (!navigator.gpu) { return false; }
    var adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
    if (!adapter) { return 'requestAdapter returned null'; }
    device = await adapter.requestDevice();
    var limits = adapter.limits;
    Probe.info('adapter', {
      // adapter.info is the modern replacement for the removed requestAdapterInfo().
      vendor: adapter.info ? adapter.info.vendor : 'unknown',
      architecture: adapter.info ? adapter.info.architecture : 'unknown',
      device: adapter.info ? adapter.info.device : 'unknown',
      features: Array.from(adapter.features || [])
    });
    Probe.info('limits', {
      maxBufferSize: limits.maxBufferSize,
      maxBufferSizeGB: Math.round((limits.maxBufferSize || 0) / 1e9 * 100) / 100,
      maxStorageBufferBindingSize: limits.maxStorageBufferBindingSize,
      maxComputeWorkgroupStorageSize: limits.maxComputeWorkgroupStorageSize,
      maxComputeInvocationsPerWorkgroup: limits.maxComputeInvocationsPerWorkgroup
    });
    // Constraint V4 says buffers cap around 2 GB. This is where that gets confirmed or
    // refuted on real hardware, and it decides the top usable model tier.
    Probe.info('tier_ceiling_hint',
      (limits.maxBufferSize || 0) >= 2e9 ? 'T2 or higher plausible' : 'T1 ceiling likely');
    return true;
  });

  // 3. Run an actual compute shader. An adapter that exists but cannot dispatch is worse
  //    than no adapter, because the tiering logic would trust it.
  await Probe.check('compute_shader_dispatch', async function () {
    if (!device) { return false; }
    var N = 1024;
    var shader = device.createShaderModule({
      code: [
        '@group(0) @binding(0) var<storage, read_write> data: array<f32>;',
        '@compute @workgroup_size(64)',
        'fn main(@builtin(global_invocation_id) gid: vec3<u32>) {',
        '  data[gid.x] = data[gid.x] * 2.0 + 1.0;',
        '}'
      ].join('\n')
    });

    var buffer = device.createBuffer({
      size: N * 4,
      usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST
    });
    var input = new Float32Array(N);
    for (var i = 0; i < N; i++) { input[i] = i; }
    device.queue.writeBuffer(buffer, 0, input);

    var pipeline = device.createComputePipeline({
      layout: 'auto',
      compute: { module: shader, entryPoint: 'main' }
    });
    var bindGroup = device.createBindGroup({
      layout: pipeline.getBindGroupLayout(0),
      entries: [{ binding: 0, resource: { buffer: buffer } }]
    });

    var encoder = device.createCommandEncoder();
    var pass = encoder.beginComputePass();
    pass.setPipeline(pipeline);
    pass.setBindGroup(0, bindGroup);
    pass.dispatchWorkgroups(N / 64);
    pass.end();

    var readback = device.createBuffer({
      size: N * 4,
      usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ
    });
    encoder.copyBufferToBuffer(buffer, 0, readback, 0, N * 4);
    device.queue.submit([encoder.finish()]);

    await readback.mapAsync(GPUMapMode.READ);
    var out = new Float32Array(readback.getMappedRange().slice(0));
    readback.unmap();

    // data[i] = i * 2 + 1, so index 100 must be exactly 201.
    return out[100] === 201 ? true : 'expected 201 at index 100, got ' + out[100];
  });

  // 4. Allocate a 1 GB buffer — the smallest allocation a Tier 2 model needs, and the point
  //    where an adapter's advertised limits often stop being true.
  await Probe.check('one_gb_buffer_allocation', async function () {
    if (!device) { return false; }
    try {
      var big = device.createBuffer({ size: 1024 * 1024 * 1024, usage: GPUBufferUsage.STORAGE });
      big.destroy();
      return true;
    } catch (err) {
      return 'allocation failed: ' + err.message;
    }
  });

  // 5. Workers via Blob URL. A classic worker from file:// is blocked by the null origin,
  //    so the Blob escape hatch is the only route — and index work cannot run on the main
  //    thread without freezing the UI.
  await Probe.check('blob_worker_runs', function () {
    return new Promise(function (resolve) {
      var src = 'self.onmessage = function (e) { self.postMessage(e.data * 2); };';
      var url = URL.createObjectURL(new Blob([src], { type: 'text/javascript' }));
      var w;
      try { w = new Worker(url); }
      catch (err) { resolve('Worker construction threw: ' + err.message); return; }
      w.onmessage = function (e) { w.terminate(); resolve(e.data === 42); };
      w.onerror = function (err) { w.terminate(); resolve('worker error: ' + err.message); };
      w.postMessage(21);
      setTimeout(function () { resolve('worker timed out'); }, 5000);
    });
  });

  // 6. Can a Blob worker itself reach WebGPU? If not, inference is stuck on the main thread
  //    and the UI must be designed around that.
  await Probe.check('webgpu_inside_blob_worker', function () {
    return new Promise(function (resolve) {
      var src = [
        'self.onmessage = async function () {',
        '  if (typeof navigator.gpu === "undefined") { self.postMessage({ok:false, why:"no navigator.gpu"}); return; }',
        '  try {',
        '    const a = await navigator.gpu.requestAdapter();',
        '    self.postMessage({ ok: !!a, why: a ? "adapter acquired" : "adapter null" });',
        '  } catch (e) { self.postMessage({ ok:false, why: String(e.message) }); }',
        '};'
      ].join('\n');
      var url = URL.createObjectURL(new Blob([src], { type: 'text/javascript' }));
      var w;
      try { w = new Worker(url); }
      catch (err) { resolve('Worker construction threw: ' + err.message); return; }
      w.onmessage = function (e) { w.terminate(); resolve(e.data.ok ? e.data : e.data.why); };
      w.onerror = function (err) { w.terminate(); resolve('worker error: ' + err.message); };
      w.postMessage('go');
      setTimeout(function () { resolve('worker timed out'); }, 10000);
    });
  });

  // 7. Confirm what the null origin costs: no cross-origin isolation means no
  //    SharedArrayBuffer, hence no multi-threaded WASM on the CPU. Recorded as evidence
  //    for constraint V2 rather than as a failure.
  await Probe.check('sab_status_under_null_origin', function () {
    return {
      crossOriginIsolated: self.crossOriginIsolated,
      sharedArrayBuffer: typeof SharedArrayBuffer !== 'undefined',
      note: 'absence here is expected under file:// and is why CPU inference stays single-threaded'
    };
  });

  Probe.finish();
}());
</script>
```

- [ ] **Step 2: Eseguire controllo e condizione reale, su ogni browser e OS**

```bash
cd ~/Desktop/SwissBunker/spikes/phase-0
./tools/serve.sh &
open -a "Google Chrome" http://localhost:8000/p5-webgpu-worker.html
open -a "Google Chrome" ~/Desktop/SwissBunker/spikes/phase-0/p5-webgpu-worker.html
```

Criteri: `webgpu_device_acquired`, `compute_shader_dispatch` e `blob_worker_runs` devono
passare da `file://` su almeno Chrome ed Edge. Se `webgpu_inside_blob_worker` fallisce
ovunque, l'inferenza resta sul thread principale e la UI va progettata di conseguenza.

- [ ] **Step 3: Raccogliere i limiti hardware su almeno tre macchine diverse**

Servono un Apple Silicon, un portatile con GPU integrata Intel o AMD, e una macchina con GPU
discreta. Sono questi tre punti a calibrare la tabella dei tier di §8.1, che al momento è
una stima.

- [ ] **Step 4: Committare**

```bash
cd ~/Desktop/SwissBunker
git add spikes/phase-0
git commit -m "spike(phase-0): P5 WebGPU and Blob workers under null origin"
```

---

### Task 6: P6 — Pesi del modello da oggetti `File`

**Files:**
- Create: `spikes/phase-0/p6-model-weights.html`

**Interfaces:**
- Consumes: `window.Probe` (Task 0); i limiti hardware di P5 (Task 5)
- Produces: il verdetto sul **rischio R2**. Determina quanto lavoro di fork serve per far
  caricare a una libreria di inferenza dei pesi che sono già sul disco.

**Il problema, in una riga:** WebLLM e wllama sono progettate per *scaricare* i pesi via HTTP
e metterli in Cache API. Qui i pesi sono già presenti come oggetti `File`. Questo probe
misura la distanza tra le due cose.

- [ ] **Step 1: Procurarsi i pesi di un modello piccolo**

```bash
mkdir -p ~/swissbunker-fixtures/model
cd ~/swissbunker-fixtures/model
# A small GGUF is enough: this probe measures the loading path, not answer quality.
curl -fL --retry 3 -O \
  "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf"
ls -lh
cp ./*.gguf /Volumes/SWISSTEST/ || echo "mount SWISSTEST first"
```

- [ ] **Step 2: Scrivere `spikes/phase-0/p6-model-weights.html`**

```html
<!doctype html>
<meta charset="utf-8">
<title>P6</title>
<style>body{font:14px/1.6 ui-monospace,monospace;margin:2rem;max-width:80ch}pre{background:#f4f4f5;padding:1rem;border-radius:8px;overflow-x:auto}</style>
<h1>P6 — Model weights from File objects</h1>
<p>Select the volume containing the <code>.gguf</code> file.</p>
<input type="file" id="dir-input" webkitdirectory multiple>
<pre id="probe-output"></pre>
<script src="lib/probe.js"></script>
<script>
(function () {
  Probe.init('p6', 'Model weights from File objects');

  document.getElementById('dir-input').addEventListener('change', async function (e) {
    var files = Array.prototype.slice.call(e.target.files);
    var gguf = files.filter(function (f) { return /\.gguf$/i.test(f.name); })[0];

    if (!gguf) {
      await Probe.check('weights_found', function () { return false; });
      Probe.finish();
      return;
    }

    await Probe.check('weights_found', function () { return true; });
    Probe.info('weights', { name: gguf.name, bytes: gguf.size,
                            GB: Math.round(gguf.size / 1e9 * 100) / 100 });

    // 1. Parse the GGUF header from a range read. GGUF starts with the magic "GGUF"
    //    followed by a little-endian uint32 version. Reading structure — not just bytes —
    //    from the file is what makes a streaming loader possible.
    await Probe.check('gguf_header_parsed', async function () {
      var buf = await gguf.slice(0, 8).arrayBuffer();
      var dv = new DataView(buf);
      var magic = String.fromCharCode(dv.getUint8(0), dv.getUint8(1), dv.getUint8(2), dv.getUint8(3));
      var version = dv.getUint32(4, true);
      Probe.info('gguf', { magic: magic, version: version });
      return magic === 'GGUF';
    });

    // 2. Throughput of streaming the file in 64 MB chunks. This becomes "how long before
    //    the model answers" — the dominant term in NF1's 90-second budget.
    await Probe.measure('stream_512mb_ms', async function () {
      var CHUNK = 64 * 1024 * 1024;
      var limit = Math.min(gguf.size, 512 * 1024 * 1024);
      for (var off = 0; off < limit; off += CHUNK) {
        await gguf.slice(off, Math.min(off + CHUNK, limit)).arrayBuffer();
      }
    }, 5);

    // 3. Can the whole file be held at once? This is the naive path every inference library
    //    takes by default, and the one that fails on a 2.5 GB Tier 2 model against the
    //    4 GB WASM32 heap of constraint V4.
    await Probe.check('whole_file_in_memory', async function () {
      if (gguf.size > 1.5e9) { return 'skipped: file too large to risk on the main thread'; }
      try {
        var buf = await gguf.arrayBuffer();
        var ok = buf.byteLength === gguf.size;
        buf = null;
        return ok;
      } catch (err) {
        return 'failed: ' + err.message;
      }
    });

    // 4. Does the file survive the Cache API? If a File can be stashed as a Response, then
    //    libraries expecting a cached HTTP fetch can be satisfied with almost no fork.
    //    This is the cheap path for risk R2, so it is tested before the expensive one.
    await Probe.check('cache_api_accepts_file', async function () {
      if (typeof caches === 'undefined') { return 'Cache API unavailable (expected under file://)'; }
      try {
        var cache = await caches.open('probe-weights');
        await cache.put(new Request('https://local/weights.gguf'),
                        new Response(gguf.slice(0, 1024 * 1024)));
        var hit = await cache.match('https://local/weights.gguf');
        var got = hit ? (await hit.arrayBuffer()).byteLength : 0;
        await caches.delete('probe-weights');
        return got === 1024 * 1024;
      } catch (err) {
        return 'failed: ' + err.message;
      }
    });

    // 5. Can a File be handed to a Worker without copying it? Structured cloning a File
    //    should transfer the handle, not the bytes — if it copies, loading a 2.5 GB model
    //    inside a worker doubles peak memory and blows the heap.
    await Probe.check('file_transfers_to_worker', function () {
      return new Promise(function (resolve) {
        var src = [
          'self.onmessage = async function (e) {',
          '  const f = e.data;',
          '  try {',
          '    const head = await f.slice(0, 4).arrayBuffer();',
          '    const dv = new DataView(head);',
          '    const magic = String.fromCharCode(dv.getUint8(0), dv.getUint8(1), dv.getUint8(2), dv.getUint8(3));',
          '    self.postMessage({ ok: magic === "GGUF", magic: magic, size: f.size });',
          '  } catch (err) { self.postMessage({ ok: false, why: String(err.message) }); }',
          '};'
        ].join('\n');
        var url = URL.createObjectURL(new Blob([src], { type: 'text/javascript' }));
        var w;
        try { w = new Worker(url); }
        catch (err) { resolve('Worker construction threw: ' + err.message); return; }
        w.onmessage = function (ev) { w.terminate(); resolve(ev.data); };
        w.onerror = function (err) { w.terminate(); resolve('worker error: ' + err.message); };
        w.postMessage(gguf);
        setTimeout(function () { resolve('worker timed out'); }, 10000);
      });
    });

    Probe.finish();
  });
}());
</script>
```

- [ ] **Step 3: Eseguire e classificare l'esito su R2**

```bash
cd ~/Desktop/SwissBunker/spikes/phase-0
./tools/serve.sh &
open -a "Google Chrome" http://localhost:8000/p6-model-weights.html
open -a "Google Chrome" ~/Desktop/SwissBunker/spikes/phase-0/p6-model-weights.html
```

| Esito | Significato per R2 | Lavoro stimato in Fase 4 |
|---|---|---|
| `cache_api_accepts_file` verde | Si può soddisfare il loader della libreria pre-popolando la Cache API | Basso — poche centinaia di righe |
| `cache_api` rosso, `file_transfers_to_worker` verde | Serve un loader custom, ma i `File` raggiungono il worker senza copia | Medio — fork contenuto |
| Entrambi rossi | Il caricamento va riscritto contro il runtime di inferenza | Alto — rivalutare `wllama` contro WebLLM |

- [ ] **Step 4: Committare**

```bash
cd ~/Desktop/SwissBunker
git add spikes/phase-0
git commit -m "spike(phase-0): P6 model weight loading from File objects, risk R2 evidence"
```

---

### Task 7: Aggregazione e decisione go/no-go

**Files:**
- Create: `spikes/phase-0/report.html`
- Create: `docs/reports/2026-XX-XX-phase-0-findings.md`
- Modify: `docs/specs/2026-08-18-swissbunker-design.md` (§14, probabilità misurate al posto
  di quelle stimate)
- Modify: `README.md` (riga `Status`)

**Interfaces:**
- Consumes: tutti i file JSON in `spikes/phase-0/results/`
- Produces: **il deliverable della Fase 0** — la decisione documentata, con le prove.

- [ ] **Step 1: Scrivere `spikes/phase-0/report.html`**

```html
<!doctype html>
<meta charset="utf-8">
<title>Phase 0 — Results matrix</title>
<style>
body{font:14px/1.6 ui-monospace,monospace;margin:2rem}
table{border-collapse:collapse;margin:1rem 0;font-size:13px}
th,td{border:1px solid #d4d4d8;padding:.4rem .7rem;text-align:left;vertical-align:top}
th{background:#f4f4f5}
.ok{background:#dcfce7}.fail{background:#fee2e2}.na{background:#f4f4f5;color:#71717a}
</style>
<h1>Phase 0 — Results matrix</h1>
<p>Select every JSON file in <code>results/</code>.</p>
<input type="file" id="files" multiple accept=".json">
<div id="out"></div>
<script>
document.getElementById('files').addEventListener('change', async function (e) {
  var records = [];
  for (var i = 0; i < e.target.files.length; i++) {
    var f = e.target.files[i];
    try {
      var rec = JSON.parse(await f.text());
      // The operator encodes browser and OS in the filename; the harness cannot know them.
      var parts = f.name.replace(/\.json$/, '').split('-');
      rec._file = f.name;
      rec._protocol = parts[1] || '?';
      rec._browser = parts[2] || '?';
      rec._os = parts[3] || '?';
      records.push(rec);
    } catch (err) {
      console.warn('skipping unparseable file', f.name, err);
    }
  }

  // One table per probe: rows are checks, columns are browser/OS/protocol combinations.
  var byProbe = {};
  records.forEach(function (r) { (byProbe[r.probe] = byProbe[r.probe] || []).push(r); });

  var html = '';
  Object.keys(byProbe).sort().forEach(function (probe) {
    var recs = byProbe[probe];
    var checkNames = {};
    recs.forEach(function (r) { Object.keys(r.checks).forEach(function (k) { checkNames[k] = 1; }); });

    html += '<h2>' + probe + ' — ' + recs[0].title + '</h2><table><tr><th>check</th>';
    recs.forEach(function (r) {
      html += '<th>' + r._browser + '<br>' + r._os + '<br><em>' + r._protocol + '</em></th>';
    });
    html += '</tr>';

    Object.keys(checkNames).sort().forEach(function (name) {
      html += '<tr><td>' + name + '</td>';
      recs.forEach(function (r) {
        var c = r.checks[name];
        if (!c) { html += '<td class="na">—</td>'; return; }
        var cls = c.ok ? 'ok' : 'fail';
        var label = c.ok ? 'PASS' : 'FAIL';
        var detail = typeof c.detail === 'string' ? c.detail : '';
        html += '<td class="' + cls + '" title="' + detail.replace(/"/g, '&quot;') + '">'
             + label + (detail ? '<br><small>' + detail.slice(0, 40) + '</small>' : '') + '</td>';
      });
      html += '</tr>';
    });
    html += '</table>';

    // Measurements get their own table: p50 and p95 are the numbers the design budget needs.
    var measureNames = {};
    recs.forEach(function (r) { Object.keys(r.measurements).forEach(function (k) { measureNames[k] = 1; }); });
    if (Object.keys(measureNames).length) {
      html += '<table><tr><th>measurement (p50 / p95 ms)</th>';
      recs.forEach(function (r) { html += '<th>' + r._browser + ' ' + r._protocol + '</th>'; });
      html += '</tr>';
      Object.keys(measureNames).sort().forEach(function (name) {
        html += '<tr><td>' + name + '</td>';
        recs.forEach(function (r) {
          var m = r.measurements[name];
          html += m ? '<td>' + m.p50 + ' / ' + m.p95 + '</td>' : '<td class="na">—</td>';
        });
        html += '</tr>';
      });
      html += '</table>';
    }
  });

  document.getElementById('out').innerHTML = html || '<p>No parseable records selected.</p>';
});
</script>
```

- [ ] **Step 2: Aprire il report e generare la matrice**

```bash
open ~/Desktop/SwissBunker/spikes/phase-0/report.html
```

Selezionare tutti i file in `results/`. Salvare uno screenshot della matrice in
`docs/reports/assets/`.

- [ ] **Step 3: Scrivere il documento di findings**

Creare `docs/reports/<data>-phase-0-findings.md` con questa struttura esatta:

````markdown
# Fase 0 — Findings e decisione

**Data:** <data di completamento>
**Esecuzioni:** <n> record, <n> browser, <n> sistemi operativi
**Decisione:** GO / GO CON MODIFICHE / NO-GO

## Sommario

<Tre frasi. La prima dice se la modalità Portable è realizzabile. La seconda dice qual è il
vincolo più stringente emerso. La terza dice cosa cambia nella spec.>

## Matrice dei risultati

<La tabella generata da report.html, incollata come markdown.>

## Verdetti per rischio

| Rischio | Probabilità stimata nella spec | Esito misurato | Nuova probabilità |
|---|---|---|---|
| R1 — `file://` blocca troppe API | Media | | |
| R2 — pesi LLM da `FileList` | Media | | |
| R6 — `webkitdirectory` oltre i 4 GB | Media | | |
| R7 — limite `maxBufferSize` WebGPU | Media | | |

## Budget di latenza misurato

| Misura | Valore p95 | Budget di riferimento | Margine |
|---|---|---|---|
| Lettura contigua 5.6 MB (scan IVF) | | NF2: 800 ms totali | |
| Query FTS5 simulata | | NF2: 800 ms totali | |
| Streaming pesi 512 MB | | NF1: 90 s totali | |

## Conferma o smentita delle scelte architetturali

- **IVF invece di HNSW** — rapporto misurato tra `scattered_1400x4kb_ms` e
  `contiguous_5_6mb_ms`: <valore>. <Conferma o smentisce §6.3.>
- **`webkitdirectory` invece di File System Access** — <esito per browser.>
- **Tiering dei modelli** — `maxBufferSize` osservato su <n> macchine: <valori>.
  <La tabella di §8.1 va aggiornata così: ...>

## Modifiche richieste alla specifica

<Elenco puntuale, ognuna con la sezione da modificare.>

## Cosa serve prima della Fase 1

<Elenco puntuale.>
````

- [ ] **Step 4: Aggiornare la spec con le probabilità misurate**

Nel registro rischi §14 della spec, sostituire i valori della colonna `Prob.` con quelli
misurati e aggiungere una colonna `Evidenza` che punta al documento di findings.

- [ ] **Step 5: Aggiornare lo stato nel README**

```bash
cd ~/Desktop/SwissBunker
sed -i '' 's|`Status: design phase`|`Status: phase 0 complete`|' README.md
grep -n 'Status:' README.md
```

- [ ] **Step 6: Commit finale e tag**

```bash
cd ~/Desktop/SwissBunker
git add -A
git commit -m "docs(phase-0): findings, risk re-assessment, go/no-go decision"
git tag -a phase-0-complete -m "Phase 0 feasibility gate"
git push origin main --tags
```

---

## Criteri di uscita della Fase 0

La fase è chiusa quando tutte queste condizioni sono vere:

1. I sei probe sono stati eseguiti su almeno **4 browser × 2 sistemi operativi**, in entrambi
   i protocolli, e i record sono committati in `results/`.
2. P3 è stato eseguito su un **disco esterno reale**, non solo sull'immagine exFAT.
3. Ogni rischio fra R1, R2, R6 e R7 ha una probabilità **misurata** al posto di quella stimata.
4. Il documento di findings esiste e contiene una decisione esplicita fra GO, GO CON
   MODIFICHE e NO-GO.
5. Se la decisione è GO CON MODIFICHE, la spec è aggiornata **prima** che inizi la Fase 1.
6. Se la decisione è NO-GO, il documento indica quale piano B della spec §14 viene adottato,
   e la spec viene riscritta di conseguenza prima di qualsiasi altro lavoro.

## Cosa NON fa questa fase

- Non scrive codice di produzione. Niente in `spikes/` può essere importato dopo.
- Non sceglie librerie definitive. Misura quanto costerebbe adottarle.
- Non ottimizza nulla. Un probe lento che misura correttamente è un probe riuscito.
