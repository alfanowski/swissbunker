# Fase 1 — Reader · Piano di implementazione

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Una pagina aperta da `file://` interroga un indice SQLite FTS5 multi-gigabyte su un
disco esterno e mostra risultati di ricerca reali, senza caricare l'indice in memoria e senza
installare nulla sulla macchina ospite.

**Architecture:** Quattro strati sovrapposti, ciascuno testabile da solo. Una **sorgente**
astrae da dove arrivano i byte; un **lettore sincrono** li rende disponibili in modo
bloccante, unica forma che SQLite accetti; una **cache a pagine** con eviction LRU limita
l'amplificazione di lettura; un **VFS** collega il tutto a SQLite. Sopra ci sta la ricerca
FTS5 e una UI minima.

**Tech Stack:** TypeScript, **`@sqlite.org/sqlite-wasm` 3.53.0** (deciso nel Task 1, già
eseguito), esbuild per il bundle IIFE, Vitest in browser mode per i test unitari, Playwright
per la suite di conformità `file://`.

**Spec:** [`docs/specs/2026-08-18-swissbunker-design.md`](../specs/2026-08-18-swissbunker-design.md) — §4.2 `reader`, §6.4, §6.3
**Evidenza di partenza:** [`docs/reports/2026-08-19-phase-0-findings.md`](../reports/2026-08-19-phase-0-findings.md)

---

## Global Constraints

Ereditati dalla Fase 0 e ora misurati, non più supposti:

- **Nessun modulo ES a runtime.** L'import dinamico e `fetch` di un file vicino falliscono da
  `file://` su Chromium, Chrome e WebKit. Tutto il codice spedito è un **bundle IIFE**, e il
  wasm viaggia inlineato in base64 passato via `wasmBinary`.
- **Niente OPFS.** Negato da `file://` su Chromium e WebKit (vincolo V7). Lo stato utente sta
  in **IndexedDB**, che passa su tutti e quattro i motori.
- **Niente `SharedArrayBuffer`, niente WASM multi-thread su CPU.** `crossOriginIsolated` è
  falso sotto origin nullo.
- **Le letture sincrone passano da XHR sincrona su Blob URL.** Misurate a 0.7 ms per 4 KB su
  tutti e quattro i motori. È l'unico meccanismo bloccante disponibile.
- **Pattern di lettura contiguo, non sparso.** Letture sparse costano 39× le contigue a parità
  di byte. Ogni struttura dati nuova deve leggere in blocchi contigui.
- **Il lettore è sequenziale.** Le letture parallele hanno reso solo 1.23×: non vale la
  complessità.
- **Budget:** ricerca completa sotto gli **800 ms p95** (NF2). Obiettivo interno di fase:
  **500 ms** su Wikipedia IT.
- Codice, identificatori e commenti in inglese. Documentazione di piano in italiano.

### Regola metodologica, appresa in Fase 0

> **Non scrivere codice contro un'API che non hai ispezionato.**
> Il piano della Fase 0 conteneva chiamate a `FS.createLazyFile` con una firma inventata: quel
> metodo non esiste nel build usato. Ogni task che tocca un'API di terze parti **inizia** con
> uno step che la stampa e la verifica, e solo dopo scrive codice.

---

## Struttura dei file

| File | Responsabilità |
|---|---|
| `web/package.json` | Dipendenze e script |
| `web/build.mjs` | Bundle IIFE con wasm inlineato |
| `web/vitest.config.ts` | Test unitari in browser mode |
| `web/src/io/source.ts` | `ByteSource` e le sue due implementazioni, inclusa la lettura **sincrona** bloccante via XHR su Blob URL |
| `web/src/io/page-cache.ts` | Cache a pagine con eviction LRU e contabilità |
| `web/src/sqlite/vfs.ts` | VFS SQLite sopra la cache |
| `web/src/sqlite/open.ts` | Init di SQLite e apertura del database |
| `web/src/search/fts.ts` | Query FTS5, snippet, ranking |
| ~~`web/src/zim/reader.ts`~~ | **Rinviato alla Fase 2** — vedi nota sotto |
| `web/src/ui/app.ts` | UI minima: input, risultati, stato |
| `web/test/unit/*.test.ts` | Test unitari, girano su `http://` |
| `web/test/conformance/*.html` | Suite `file://`, guidata da Playwright |
| `web/tools/build-test-index.py` | Costruttore di indice di prova — **impalcatura, non la Forge** |

**Perché il lettore ZIM non è in questa fase**, benché la spec §13 lo elencasse qui:
l'indice della Fase 1 conserva il testo degli articoli, quindi titoli e snippet arrivano da
FTS5 e il criterio di uscita si verifica senza mai aprire uno ZIM. Il lettore ZIM diventa
necessario solo quando l'indice passa a contentless per risparmiare spazio, che è una
decisione della Fase 2. Costruirlo ora significherebbe scriverlo prima di sapere quale
interfaccia gli servirà.

**Perché `ByteSource` esiste.** I test unitari non possono fabbricare un `File` da 6 GB, e la
produzione non può usare un `ArrayBuffer`. Un'interfaccia sola con due implementazioni rende
la logica testabile senza fixture giganti, e lascia il percorso reale identico a quello
testato. Senza di essa ogni test avrebbe bisogno di un disco.

**Perché due suite di test.** I test unitari girano su `http://` sotto Vitest, dove si
ricaricano in un secondo — ma `http://` è la condizione *sbagliata*: è il controllo, non il
bersaglio. La suite di conformità gira da `file://`, è lenta, e contiene solo le asserzioni
che perdono senso su `http://`. Tenere le due cose separate evita sia i test lenti sia i test
che passano nella condizione che non ci interessa.

---

### Task 1: Decidere il motore SQLite — spike breve · ✅ COMPLETATO 2026-08-19

> **Esito: `@sqlite.org/sqlite-wasm`.** Entrambi i candidati aprono un database attraverso un
> VFS custom; solo l'ufficiale ha FTS5 — `wa-sqlite` non lo contiene in nessuno dei suoi due
> build wasm. Il VFS di sola lettura costa ~105 righe e ha risposto alla query needle leggendo
> il **2.1% del database**. Dettagli e tabella comparativa in
> [`web/spike-vfs/README.md`](../../web/spike-vfs/README.md).
>
> I passi sotto restano come traccia di ciò che è stato fatto. Il Task 2 è il prossimo.

**Files:**
- Create: `web/spike-vfs/README.md`
- Create: `web/spike-vfs/inspect-api.mjs`

**Interfaces:**
- Consumes: niente
- Produces: la decisione fra `@sqlite.org/sqlite-wasm` e `wa-sqlite`, con la firma **reale**
  dei metodi VFS che i Task successivi devono implementare.

**Perché questa decisione non è chiusa.** La Fase 0 ha scelto il pacchetto ufficiale perché
espone `sqlite3_vfs_register`, e questa parte resta valida. Ma *esporre* la funzione C e
*rendere pratico* scrivere un VFS sono cose diverse: nel pacchetto ufficiale un VFS custom si
costruisce allocando e popolando struct C dentro la memoria WASM, mentre `wa-sqlite` è
progettato apposta perché si estenda una classe JavaScript con metodi `xRead`, `xOpen`,
`xFileSize`. La Fase 0 non ha valutato `wa-sqlite`, quindi la scelta è informata a metà.
Questo spike la chiude prima che qualcuno ci costruisca sopra tre settimane di lavoro.

- [ ] **Step 1: Installare entrambi i candidati**

```bash
cd ~/Desktop/SwissBunker
mkdir -p web/spike-vfs && cd web/spike-vfs
npm init -y >/dev/null
npm install --no-audit --no-fund @sqlite.org/sqlite-wasm@3.53.0-build1 wa-sqlite@1.0.0
```

- [ ] **Step 2: Stampare l'API reale di entrambi, senza scrivere codice contro di essa**

```javascript
// web/spike-vfs/inspect-api.mjs
// Print what each library actually offers for building a custom VFS. This runs BEFORE any
// implementation is written, because Phase 0 was burned by coding against an invented
// signature. Nothing here is kept.
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);

console.log('=== @sqlite.org/sqlite-wasm ===');
const mod = await import('@sqlite.org/sqlite-wasm');
const sqlite3 = await mod.default();
console.log('  version:', sqlite3.version.libVersion);
const vfsApi = Object.keys(sqlite3.capi).filter(k => /vfs|_io_|file/i.test(k)).sort();
console.log('  vfs-related capi symbols:', vfsApi.length);
vfsApi.forEach(k => console.log('    ', k));
// The struct binding layer is what decides whether a JS VFS is practical here.
console.log('  StructBinder:', typeof sqlite3.StructBinder);
if (sqlite3.StructBinder) {
  console.log('  struct types:', Object.keys(sqlite3.StructBinder).slice(0, 20).join(', '));
}
console.log('  vfs helper namespace:', typeof sqlite3.vfs, Object.keys(sqlite3.vfs || {}));

console.log('\n=== wa-sqlite ===');
try {
  const wa = require('wa-sqlite');
  const factory = require('wa-sqlite/dist/wa-sqlite.node.mjs');
  console.log('  exports:', Object.keys(wa).slice(0, 20).join(', '));
  // The whole point of wa-sqlite: a base class you subclass with xRead/xOpen/xFileSize.
  const base = require('wa-sqlite/src/VFS.js');
  const proto = base.Base ? Object.getOwnPropertyNames(base.Base.prototype) : [];
  console.log('  VFS.Base methods:', proto.join(', '));
} catch (e) {
  console.log('  inspection failed:', e.message.split('\n')[0]);
}
```

```bash
node inspect-api.mjs 2>&1 | tee api-surface.txt
```

- [ ] **Step 3: Implementare il VFS minimo che apre un database, in entrambi**

Per ciascun candidato, il criterio è identico e binario: **un database su disco viene aperto e
interrogato attraverso letture che passano tutte dal VFS custom**, contate. Non serve
correttezza completa: servono `xOpen`, `xRead`, `xFileSize`, `xClose` e i lock in no-op.

Al termine, registrare in `web/spike-vfs/README.md`:

| | `@sqlite.org/sqlite-wasm` | `wa-sqlite` |
|---|---|---|
| Righe di codice per il VFS minimo | | |
| Il VFS ha ricevuto tutte le letture | | |
| FTS5 presente | sì (Fase 0) | |
| Dimensione del bundle | | |
| Ore stimate per il VFS completo | | |

- [ ] **Step 4: Decidere e scriverlo nella spec**

Aggiornare §12 e la riga R10 di §14 con la scelta e la ragione. Poi:

```bash
cd ~/Desktop/SwissBunker
git add web/spike-vfs docs/specs
git commit -m "spike(phase-1): choose the SQLite engine on VFS ergonomics, not just features"
```

**Il resto del piano dice `SqliteEngine` dove la scelta è indifferente**, e nomina il
pacchetto solo dove conta.

---

### Task 2: `ByteSource` — l'astrazione della sorgente

**Files:**
- Create: `web/package.json`, `web/tsconfig.json`, `web/vitest.config.ts`
- Create: `web/src/io/source.ts`
- Create: `web/test/unit/source.test.ts`

**Interfaces:**
- Consumes: niente
- Produces:
  ```typescript
  interface ByteSource {
    readonly size: number;
    readonly name: string;
    /** Asynchronous range read. Always available. */
    read(offset: number, length: number): Promise<Uint8Array>;
    /** Synchronous range read, or null when the source cannot block. */
    readSync(offset: number, length: number): Uint8Array | null;
  }
  class FileSource implements ByteSource      // production: wraps a File
  class BufferSource implements ByteSource    // tests: wraps a Uint8Array
  ```

- [ ] **Step 1: Impalcatura del progetto**

```bash
cd ~/Desktop/SwissBunker/web
npm init -y >/dev/null
npm install --no-audit --no-fund -D typescript@5.7.2 vitest@2.1.8 @vitest/browser@2.1.8 \
  playwright@1.49.1 esbuild@0.24.0
npx playwright install chromium
```

```json
// web/tsconfig.json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "noEmit": true,
    "types": ["vitest/globals"]
  },
  "include": ["src", "test"]
}
```

```typescript
// web/vitest.config.ts
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    globals: true,
    browser: {
      enabled: true,
      provider: 'playwright',
      instances: [{ browser: 'chromium' }],
      headless: true
    }
  }
});
```

**Perché i test unitari girano nel browser e non in Node:** ogni cosa che questo codice tocca
— `Blob`, `URL.createObjectURL`, `XMLHttpRequest` sincrona, `File` — esiste solo lì. Un test
in Node passerebbe contro degli stub e non direbbe nulla.

- [ ] **Step 2: Scrivere il test che fallisce**

```typescript
// web/test/unit/source.test.ts
import { describe, it, expect } from 'vitest';
import { BufferSource, FileSource } from '../../src/io/source';

// Pages carry their own index, so a read can be checked for CONTENT, not just for length.
// Phase 0 showed silent wrong bytes are the failure mode that matters.
function makePages(count: number, pageSize = 4096): Uint8Array {
  const buf = new Uint8Array(count * pageSize);
  const enc = new TextEncoder();
  for (let i = 0; i < count; i++) {
    buf.set(enc.encode(`PAGE:${String(i).padStart(12, '0')}:`), i * pageSize);
  }
  return buf;
}

describe('BufferSource', () => {
  it('reads the exact bytes at an arbitrary offset', async () => {
    const src = new BufferSource(makePages(16), 'test.bin');
    const got = await src.read(3 * 4096, 19);
    expect(new TextDecoder().decode(got)).toBe('PAGE:000000000003:');
  });

  it('reports its size', () => {
    expect(new BufferSource(makePages(16), 'test.bin').size).toBe(16 * 4096);
  });

  it('clamps a read that runs past the end instead of throwing', async () => {
    const src = new BufferSource(makePages(2), 'test.bin');
    const got = await src.read(2 * 4096 - 10, 100);
    expect(got.length).toBe(10);
  });
});

describe('FileSource', () => {
  it('reads the exact bytes synchronously', () => {
    const file = new File([makePages(16)], 'test.bin');
    const src = new FileSource(file);
    const got = src.readSync(3 * 4096, 19);
    expect(got).not.toBeNull();
    expect(new TextDecoder().decode(got!)).toBe('PAGE:000000000003:');
  });

  it('reads the exact bytes asynchronously', async () => {
    const file = new File([makePages(16)], 'test.bin');
    const got = await new FileSource(file).read(7 * 4096, 19);
    expect(new TextDecoder().decode(got)).toBe('PAGE:000000000007:');
  });
});
```

- [ ] **Step 3: Verificare che fallisca**

```bash
cd ~/Desktop/SwissBunker/web && npx vitest run test/unit/source.test.ts
```

Atteso: FAIL, `Failed to resolve import "../../src/io/source"`.

- [ ] **Step 4: Implementare**

```typescript
// web/src/io/source.ts

/**
 * A range-readable sequence of bytes.
 *
 * Two implementations exist for one reason: production reads multi-gigabyte `File` objects
 * that no test can fabricate, while tests need buffers that no production path should use.
 * Keeping one interface means the logic above never learns which one it has.
 */
export interface ByteSource {
  readonly size: number;
  readonly name: string;
  read(offset: number, length: number): Promise<Uint8Array>;
  /** Returns null when this source cannot serve a blocking read. */
  readSync(offset: number, length: number): Uint8Array | null;
}

/** Clamp a requested range to what the source actually holds. */
function clamp(offset: number, length: number, size: number): [number, number] {
  const start = Math.max(0, Math.min(offset, size));
  const end = Math.max(start, Math.min(offset + length, size));
  return [start, end];
}

export class BufferSource implements ByteSource {
  constructor(private readonly buf: Uint8Array, readonly name: string) {}

  get size(): number { return this.buf.length; }

  async read(offset: number, length: number): Promise<Uint8Array> {
    return this.readSync(offset, length)!;
  }

  readSync(offset: number, length: number): Uint8Array {
    const [start, end] = clamp(offset, length, this.size);
    return this.buf.subarray(start, end);
  }
}

export class FileSource implements ByteSource {
  constructor(private readonly file: File) {}

  get size(): number { return this.file.size; }
  get name(): string { return this.file.name; }

  async read(offset: number, length: number): Promise<Uint8Array> {
    const [start, end] = clamp(offset, length, this.size);
    return new Uint8Array(await this.file.slice(start, end).arrayBuffer());
  }

  /**
   * Blocking read of a file range.
   *
   * SQLite's VFS demands synchronous reads; File.slice() is asynchronous; and a null origin
   * denies SharedArrayBuffer, so the usual worker + Atomics.wait bridge does not exist. What
   * remains is a synchronous XHR against a Blob URL of the slice — measured at 0.7 ms for
   * 4 KB on all four engines in Phase 0.
   *
   * responseType is forbidden on a synchronous XHR in a window context, so the bytes come
   * back through the x-user-defined text encoding, where each character maps to one byte in
   * the low 8 bits.
   */
  readSync(offset: number, length: number): Uint8Array | null {
    const [start, end] = clamp(offset, length, this.size);
    if (end <= start) { return new Uint8Array(0); }

    const url = URL.createObjectURL(this.file.slice(start, end));
    try {
      const xhr = new XMLHttpRequest();
      xhr.open('GET', url, false);
      xhr.overrideMimeType('text/plain; charset=x-user-defined');
      xhr.send(null);
      if (xhr.status !== 200 && xhr.status !== 0) { return null; }

      const text = xhr.responseText;
      const out = new Uint8Array(text.length);
      for (let i = 0; i < text.length; i++) { out[i] = text.charCodeAt(i) & 0xff; }
      return out;
    } catch {
      return null;
    } finally {
      URL.revokeObjectURL(url);
    }
  }
}
```

- [ ] **Step 5: Verificare che passi**

```bash
cd ~/Desktop/SwissBunker/web && npx vitest run test/unit/source.test.ts
```

Atteso: 5 test PASS.

- [ ] **Step 6: Commit**

```bash
cd ~/Desktop/SwissBunker
git add web
git commit -m "feat(reader): ByteSource with a blocking readSync over Blob URLs"
```

---

### Task 3: `PageCache` — LRU e contabilità

**Files:**
- Create: `web/src/io/page-cache.ts`
- Create: `web/test/unit/page-cache.test.ts`

**Interfaces:**
- Consumes: `ByteSource` (Task 2)
- Produces:
  ```typescript
  interface PageCacheOptions { pageSize?: number; maxBytes?: number; }
  interface CacheStats { hits: number; misses: number; evictions: number;
                         bytesRead: number; sourceReads: number; cachedBytes: number; }
  class PageCache {
    constructor(source: ByteSource, opts?: PageCacheOptions);
    readSync(offset: number, length: number): Uint8Array | null;
    readonly stats: CacheStats;
    clear(): void;
  }
  ```

**Cosa risolve.** La Fase 0 ha misurato **39.7 MB letti per singola query** e una cache
cresciuta a 795 MB senza mai liberare nulla (rischio R13). Entrambi i problemi hanno la stessa
origine: pagine da 1 MB e nessuna eviction. La pagina scende a 64 KB e la cache prende un
tetto.

- [ ] **Step 1: Scrivere i test che falliscono**

```typescript
// web/test/unit/page-cache.test.ts
import { describe, it, expect } from 'vitest';
import { BufferSource } from '../../src/io/source';
import { PageCache } from '../../src/io/page-cache';

function pages(count: number, pageSize = 4096): Uint8Array {
  const buf = new Uint8Array(count * pageSize);
  const enc = new TextEncoder();
  for (let i = 0; i < count; i++) {
    buf.set(enc.encode(`PAGE:${String(i).padStart(12, '0')}:`), i * pageSize);
  }
  return buf;
}

describe('PageCache', () => {
  it('returns the correct bytes', () => {
    const cache = new PageCache(new BufferSource(pages(64), 'x'), { pageSize: 65536 });
    const got = cache.readSync(5 * 4096, 19);
    expect(new TextDecoder().decode(got!)).toBe('PAGE:000000000005:');
  });

  it('serves a repeated read from cache without touching the source', () => {
    const cache = new PageCache(new BufferSource(pages(64), 'x'), { pageSize: 65536 });
    cache.readSync(0, 100);
    const after = cache.stats.sourceReads;
    cache.readSync(50, 100);
    expect(cache.stats.sourceReads).toBe(after);
    expect(cache.stats.hits).toBeGreaterThan(0);
  });

  it('serves a read spanning two pages', () => {
    const cache = new PageCache(new BufferSource(pages(64), 'x'), { pageSize: 8192 });
    // 8192-byte pages, so a read at 8180 crosses into the next page.
    const got = cache.readSync(8180, 40);
    expect(got!.length).toBe(40);
    const expected = new BufferSource(pages(64), 'x').readSync(8180, 40);
    expect(Array.from(got!)).toEqual(Array.from(expected));
  });

  it('evicts to stay under maxBytes', () => {
    const cache = new PageCache(new BufferSource(pages(512), 'x'),
                                { pageSize: 4096, maxBytes: 40960 });  // 10 pages
    for (let i = 0; i < 100; i++) { cache.readSync(i * 4096, 16); }
    expect(cache.stats.cachedBytes).toBeLessThanOrEqual(40960);
    expect(cache.stats.evictions).toBeGreaterThan(0);
  });

  it('evicts the least recently used page, not the oldest', () => {
    const cache = new PageCache(new BufferSource(pages(512), 'x'),
                                { pageSize: 4096, maxBytes: 3 * 4096 });
    cache.readSync(0, 16);           // page 0
    cache.readSync(4096, 16);        // page 1
    cache.readSync(8192, 16);        // page 2
    cache.readSync(0, 16);           // page 0 again -> now most recent
    cache.readSync(12288, 16);       // page 3 -> must evict page 1, not page 0
    const before = cache.stats.sourceReads;
    cache.readSync(0, 16);           // page 0 must still be cached
    expect(cache.stats.sourceReads).toBe(before);
  });
});
```

- [ ] **Step 2: Verificare che falliscano**

```bash
cd ~/Desktop/SwissBunker/web && npx vitest run test/unit/page-cache.test.ts
```

Atteso: FAIL, modulo non risolto.

- [ ] **Step 3: Implementare**

```typescript
// web/src/io/page-cache.ts
import type { ByteSource } from './source';

export interface PageCacheOptions {
  /**
   * Phase 0 read 39.7 MB per query with 1 MB pages: at that size a single 4 KB database page
   * drags a megabyte behind it. 64 KB keeps read-ahead useful while cutting amplification by
   * more than an order of magnitude.
   */
  pageSize?: number;
  maxBytes?: number;
}

export interface CacheStats {
  hits: number;
  misses: number;
  evictions: number;
  bytesRead: number;
  sourceReads: number;
  cachedBytes: number;
}

export class PageCache {
  private readonly pageSize: number;
  private readonly maxBytes: number;
  // A Map iterates in insertion order, which is what makes LRU cheap: delete-then-set moves
  // a key to the end, so the eviction victim is always the first key.
  private readonly pages = new Map<number, Uint8Array>();
  private readonly counters = { hits: 0, misses: 0, evictions: 0, bytesRead: 0, sourceReads: 0 };

  constructor(private readonly source: ByteSource, opts: PageCacheOptions = {}) {
    this.pageSize = opts.pageSize ?? 65536;
    this.maxBytes = opts.maxBytes ?? 128 * 1024 * 1024;
  }

  get stats(): CacheStats {
    return { ...this.counters, cachedBytes: this.pages.size * this.pageSize };
  }

  clear(): void {
    this.pages.clear();
  }

  private touch(index: number): Uint8Array | undefined {
    const page = this.pages.get(index);
    if (page === undefined) { return undefined; }
    this.pages.delete(index);
    this.pages.set(index, page);
    return page;
  }

  private load(index: number): Uint8Array | null {
    const start = index * this.pageSize;
    const bytes = this.source.readSync(start, this.pageSize);
    if (bytes === null) { return null; }

    this.counters.sourceReads++;
    this.counters.bytesRead += bytes.length;
    this.pages.set(index, bytes);

    while (this.pages.size * this.pageSize > this.maxBytes && this.pages.size > 1) {
      const oldest = this.pages.keys().next().value as number;
      this.pages.delete(oldest);
      this.counters.evictions++;
    }
    return bytes;
  }

  /**
   * Blocking read of any range, assembled from cached pages.
   * Returns null only when the underlying source cannot serve a blocking read.
   */
  readSync(offset: number, length: number): Uint8Array | null {
    if (length <= 0) { return new Uint8Array(0); }

    const first = Math.floor(offset / this.pageSize);
    const last = Math.floor((offset + length - 1) / this.pageSize);

    // The common case is a read inside one page: hand back a view, no copy.
    if (first === last) {
      const cached = this.touch(first);
      const page = cached ?? this.load(first);
      if (page === null) { return null; }
      cached ? this.counters.hits++ : this.counters.misses++;
      const from = offset - first * this.pageSize;
      return page.subarray(from, Math.min(from + length, page.length));
    }

    const out = new Uint8Array(length);
    let written = 0;
    for (let i = first; i <= last; i++) {
      const cached = this.touch(i);
      const page = cached ?? this.load(i);
      if (page === null) { return null; }
      cached ? this.counters.hits++ : this.counters.misses++;

      const pageStart = i * this.pageSize;
      const from = Math.max(0, offset - pageStart);
      const take = Math.min(page.length - from, length - written);
      if (take <= 0) { break; }
      out.set(page.subarray(from, from + take), written);
      written += take;
    }
    return written === length ? out : out.subarray(0, written);
  }
}
```

- [ ] **Step 4: Verificare che passi**

```bash
cd ~/Desktop/SwissBunker/web && npx vitest run test/unit/page-cache.test.ts
```

Atteso: 5 test PASS.

- [ ] **Step 5: Commit**

```bash
cd ~/Desktop/SwissBunker
git add web
git commit -m "feat(reader): page cache with LRU eviction, closing risk R13"
```

---

### Task 4: Il VFS SQLite

**Files:**
- Create: `web/src/sqlite/vfs.ts`
- Create: `web/src/sqlite/open.ts`
- Create: `web/test/unit/vfs.test.ts`

**Interfaces:**
- Consumes: `PageCache` (Task 3); il motore scelto nel Task 1
- Produces:
  ```typescript
  interface OpenOptions { pageSize?: number; maxBytes?: number; }  // same names as PageCacheOptions
  /** Registers a read-only VFS backed by `source` and opens the database on it. */
  function openDatabase(source: ByteSource, opts?: OpenOptions): Promise<ReaderDatabase>;
  interface ReaderDatabase {
    query<T = unknown[]>(sql: string, params?: unknown[]): T[];
    close(): void;
    readonly stats: CacheStats;
  }
  ```

**Il VFS è di sola lettura.** Nessuna scrittura, nessun journal, nessun lock: il bunker è
immutabile in modalità Portable. Questo elimina i due terzi più difficili di un VFS. I metodi
di scrittura restituiscono `SQLITE_READONLY`, quelli di lock sono no-op.

**Prima di scrivere questo file, rileggere `web/spike-vfs/api-surface.txt`** e adattare le
firme a quelle reali. La struttura sotto è corretta nella logica; i nomi dei metodi vanno
verificati, non copiati alla cieca.

- [ ] **Step 1: Scrivere il test end-to-end che fallisce**

```typescript
// web/test/unit/vfs.test.ts
import { describe, it, expect, beforeAll } from 'vitest';
import { BufferSource } from '../../src/io/source';
import { openDatabase } from '../../src/sqlite/open';

// A real SQLite file built at test time by the fixture script, small enough to inline.
// The needle is planted at a known row, so the expected answer is known in advance rather
// than merely plausible — the same discipline the Phase 0 probes used.
let dbBytes: Uint8Array;

beforeAll(async () => {
  const res = await fetch('/fixtures/tiny-fts.sqlite');
  dbBytes = new Uint8Array(await res.arrayBuffer());
});

describe('openDatabase', () => {
  it('opens a database through the custom VFS', async () => {
    const db = await openDatabase(new BufferSource(dbBytes, 'tiny.sqlite'));
    expect(db).toBeDefined();
    db.close();
  });

  it('answers an FTS5 query with the planted needle', async () => {
    const db = await openDatabase(new BufferSource(dbBytes, 'tiny.sqlite'));
    const rows = db.query<[string]>(
      "SELECT title FROM docs WHERE docs MATCH 'xyzzyneedlemarker'");
    expect(rows.length).toBe(1);
    expect(rows[0]![0]).toBe('Document 13370');
    db.close();
  });

  it('routes every read through the cache rather than slurping the file', async () => {
    const db = await openDatabase(new BufferSource(dbBytes, 'tiny.sqlite'),
                                  { pageSize: 4096 });
    db.query("SELECT title FROM docs WHERE docs MATCH 'xyzzyneedlemarker'");
    // If the engine had loaded the whole file, bytesRead would equal the file size.
    expect(db.stats.sourceReads).toBeGreaterThan(0);
    expect(db.stats.bytesRead).toBeLessThan(dbBytes.length);
    db.close();
  });
});
```

- [ ] **Step 2: Generare la fixture**

```python
# web/tools/make-tiny-fixture.py
"""Build the small FTS5 database the VFS tests run against.

Deliberately tiny — a few hundred KB — so it can live in the repo and the unit suite stays
fast. The multi-gigabyte case belongs to the conformance suite, not here.
"""
import sqlite3, pathlib, random, string

out = pathlib.Path(__file__).resolve().parent.parent / "test" / "fixtures" / "tiny-fts.sqlite"
out.parent.mkdir(parents=True, exist_ok=True)
out.unlink(missing_ok=True)

con = sqlite3.connect(out)
con.execute("PRAGMA journal_mode=OFF")
con.execute("PRAGMA page_size=4096")
con.execute("CREATE VIRTUAL TABLE docs USING fts5(title, body)")
words = [''.join(random.choices(string.ascii_lowercase, k=6)) for _ in range(500)]
rows = []
for i in range(2000):
    body = ' '.join(random.choices(words, k=60))
    if i == 1337:
        body += ' xyzzyneedlemarker'
    # Row 1337 carries the title the tests assert on.
    rows.append((f'Document {13370 if i == 1337 else i}', body))
con.executemany("INSERT INTO docs(title, body) VALUES (?, ?)", rows)
con.commit()
con.execute("INSERT INTO docs(docs) VALUES('optimize')")
con.commit()
con.close()
print(f"built {out} ({out.stat().st_size // 1024} KB)")
```

```bash
cd ~/Desktop/SwissBunker/web && python3 tools/make-tiny-fixture.py
python3 -c "
import sqlite3
c = sqlite3.connect('test/fixtures/tiny-fts.sqlite')
print('needle:', c.execute(\"SELECT title FROM docs WHERE docs MATCH 'xyzzyneedlemarker'\").fetchall())"
```

Atteso: `needle: [('Document 13370',)]`. Se non è così, il resto del task misura la cosa
sbagliata.

- [ ] **Step 3: Verificare che i test falliscano**

```bash
cd ~/Desktop/SwissBunker/web && npx vitest run test/unit/vfs.test.ts
```

Atteso: FAIL, `openDatabase` non risolto.

- [ ] **Step 4: Implementare il VFS**

Adattare al motore scelto. Struttura, indipendente dal candidato:

```typescript
// web/src/sqlite/vfs.ts
import type { ByteSource } from '../io/source';
import { PageCache, type CacheStats, type PageCacheOptions } from '../io/page-cache';

/**
 * A read-only SQLite VFS backed by a PageCache.
 *
 * Read-only is not a limitation here, it is a simplification worth naming: the bunker never
 * mutates in Portable mode, so there is no journal, no write path and no locking protocol —
 * which is most of what makes a VFS hard. Writes return SQLITE_READONLY and locks are no-ops.
 *
 * Every read is synchronous, which is the whole reason FileSource.readSync exists.
 */
export class ReadOnlyVfs {
  readonly cache: PageCache;

  constructor(private readonly source: ByteSource, opts: PageCacheOptions = {}) {
    this.cache = new PageCache(source, opts);
  }

  get stats(): CacheStats { return this.cache.stats; }

  /** xRead: fill `dest` from `offset`. Short reads must be zero-filled, per the SQLite docs. */
  read(dest: Uint8Array, offset: number): number {
    const got = this.cache.readSync(offset, dest.length);
    if (got === null) { return SQLITE_IOERR_READ; }
    dest.set(got);
    if (got.length < dest.length) {
      // SQLite requires the tail zeroed and SQLITE_IOERR_SHORT_READ returned, not an error:
      // it relies on this when reading past the end of a partial page.
      dest.fill(0, got.length);
      return SQLITE_IOERR_SHORT_READ;
    }
    return SQLITE_OK;
  }

  fileSize(): number { return this.source.size; }

  write(): number { return SQLITE_READONLY; }
  truncate(): number { return SQLITE_READONLY; }
  sync(): number { return SQLITE_OK; }
  lock(): number { return SQLITE_OK; }
  unlock(): number { return SQLITE_OK; }
  checkReservedLock(): number { return SQLITE_OK; }
  /** Immutable file: tell SQLite so it skips change-counter checks. */
  deviceCharacteristics(): number { return SQLITE_IOCAP_IMMUTABLE; }
}

export const SQLITE_OK = 0;
export const SQLITE_READONLY = 8;
export const SQLITE_IOERR_READ = 266;
export const SQLITE_IOERR_SHORT_READ = 522;
export const SQLITE_IOCAP_IMMUTABLE = 0x2000;
```

- [ ] **Step 5: Verificare che i test passino**

```bash
cd ~/Desktop/SwissBunker/web && npx vitest run test/unit/vfs.test.ts
```

Atteso: 3 test PASS. Il terzo è quello che conta: prova che il database **non** viene letto
per intero.

- [ ] **Step 6: Commit**

```bash
cd ~/Desktop/SwissBunker
git add web
git commit -m "feat(reader): read-only SQLite VFS over the page cache"
```

---

### Task 5: Ricerca FTS5 con snippet e ranking

**Files:**
- Create: `web/src/search/fts.ts`
- Create: `web/test/unit/fts.test.ts`

**Interfaces:**
- Consumes: `ReaderDatabase` (Task 4)
- Produces:
  ```typescript
  interface SearchHit { docId: number; title: string; snippet: string; score: number; source: string; }
  interface SearchOptions { limit?: number; offset?: number; sources?: string[]; }
  class FtsIndex {
    constructor(db: ReaderDatabase);
    search(query: string, opts?: SearchOptions): SearchHit[];
    count(query: string): number;
  }
  ```

**Decisione da prendere qui, non altrove: la sanificazione della query.** L'input utente
finisce in un `MATCH` FTS5, che ha una sintassi propria — `"frase esatta"`, `NEAR`, `OR`,
`*`. Un apice non chiuso è un errore di sintassi, non zero risultati.

- [ ] **Step 1: Scrivere i test che falliscono**

```typescript
// web/test/unit/fts.test.ts
import { describe, it, expect, beforeAll } from 'vitest';
import { BufferSource } from '../../src/io/source';
import { openDatabase } from '../../src/sqlite/open';
import { FtsIndex } from '../../src/search/fts';

let index: FtsIndex;

beforeAll(async () => {
  const res = await fetch('/fixtures/tiny-fts.sqlite');
  const db = await openDatabase(new BufferSource(new Uint8Array(await res.arrayBuffer()), 'x'));
  index = new FtsIndex(db);
});

describe('FtsIndex', () => {
  it('finds the planted needle', () => {
    const hits = index.search('xyzzyneedlemarker');
    expect(hits.length).toBe(1);
    expect(hits[0]!.title).toBe('Document 13370');
  });

  it('returns a snippet containing the match', () => {
    const hits = index.search('xyzzyneedlemarker');
    expect(hits[0]!.snippet.toLowerCase()).toContain('xyzzyneedlemarker');
  });

  it('respects the limit', () => {
    const hits = index.search('a OR e OR i', { limit: 5 });
    expect(hits.length).toBeLessThanOrEqual(5);
  });

  it('survives an unbalanced quote instead of throwing', () => {
    expect(() => index.search('broken "quote')).not.toThrow();
  });

  it('survives FTS5 operators typed by accident', () => {
    expect(() => index.search('NEAR(')).not.toThrow();
    expect(() => index.search('*')).not.toThrow();
    expect(() => index.search('AND OR NOT')).not.toThrow();
  });

  it('returns nothing for a query with no matches', () => {
    expect(index.search('zzzznotpresentzzzz').length).toBe(0);
  });
});
```

- [ ] **Step 2: Verificare che falliscano**

```bash
cd ~/Desktop/SwissBunker/web && npx vitest run test/unit/fts.test.ts
```

- [ ] **Step 3: Implementare**

> **Punto di decisione da lasciare all'autore del progetto.** La funzione `sanitiseQuery`
> qui sotto è la sola parte di questo task dove esistono più risposte difendibili, e la scelta
> cambia il carattere del prodotto:
>
> - **Letterale** — si scappa tutto, l'utente cerca esattamente ciò che digita. Prevedibile;
>   `"frase esatta"` diventa impossibile.
> - **Permissiva** — si passa la query a FTS5 e si ricade sul letterale solo quando lancia.
>   Potente per chi conosce la sintassi, imprevedibile per gli altri.
> - **Ibrida** — si riconoscono le virgolette bilanciate e si scappa il resto.
>
> Nel contesto del bunker propendo per la **letterale con virgolette**: chi cerca "come
> depurare l'acqua" non conosce la sintassi FTS5, e un errore di sintassi davanti a una
> persona che non ha internet è peggio di una ricerca meno potente. Ma è una decisione di
> prodotto: va presa consapevolmente e annotata qui.

```typescript
// web/src/search/fts.ts
import type { ReaderDatabase } from '../sqlite/open';

export interface SearchHit {
  docId: number;
  title: string;
  snippet: string;
  score: number;
  source: string;
}

export interface SearchOptions {
  limit?: number;
  offset?: number;
  sources?: string[];
}

/**
 * Turn free user input into a valid FTS5 MATCH expression.
 *
 * FTS5 has its own grammar — quoted phrases, NEAR, OR, prefix stars — and an unbalanced
 * quote is a syntax ERROR, not an empty result set. Someone with no internet who typed a
 * stray quote must still get results.
 *
 * Strategy: every token is wrapped in double quotes so FTS5 reads it as a literal, with
 * inner quotes doubled per its escaping rule. Balanced quoted phrases in the input are
 * preserved as phrases; everything else is literal.
 */
export function sanitiseQuery(raw: string): string {
  const phrases: string[] = [];
  // Pull out balanced "quoted phrases" first, so the rest can be tokenised safely.
  const rest = raw.replace(/"([^"]+)"/g, (_m, inner: string) => {
    phrases.push(`"${inner.replace(/"/g, '""')}"`);
    return ' ';
  });

  const tokens = rest
    .split(/[\s]+/)
    .map(t => t.replace(/[^\p{L}\p{N}_*]/gu, ''))   // keep letters, digits, underscore, star
    .filter(t => t.length > 0)
    .map(t => (t.endsWith('*') ? `"${t.slice(0, -1)}"*` : `"${t}"`));

  const all = [...phrases, ...tokens];
  return all.length > 0 ? all.join(' ') : '""';     // '""' matches nothing, and never throws
}

export class FtsIndex {
  constructor(private readonly db: ReaderDatabase) {}

  search(query: string, opts: SearchOptions = {}): SearchHit[] {
    const match = sanitiseQuery(query);
    const limit = opts.limit ?? 20;
    const offset = opts.offset ?? 0;

    // bm25() returns a NEGATIVE score where more negative is better, so ordering ascending
    // puts the best match first, and negating it gives a score that reads the right way up.
    const rows = this.db.query<[number, string, string, number]>(
      `SELECT rowid,
              title,
              snippet(docs, 1, '<mark>', '</mark>', '…', 24) AS snip,
              bm25(docs) AS score
         FROM docs
        WHERE docs MATCH ?
        ORDER BY score
        LIMIT ? OFFSET ?`,
      [match, limit, offset]
    );

    return rows.map(([docId, title, snippet, score]) => ({
      docId,
      title,
      snippet,
      score: -score,
      source: 'docs'
    }));
  }

  count(query: string): number {
    const rows = this.db.query<[number]>(
      'SELECT count(*) FROM docs WHERE docs MATCH ?', [sanitiseQuery(query)]);
    return rows[0]?.[0] ?? 0;
  }
}
```

- [ ] **Step 4: Verificare che passino**

```bash
cd ~/Desktop/SwissBunker/web && npx vitest run test/unit/fts.test.ts
```

Atteso: 6 test PASS.

- [ ] **Step 5: Commit**

```bash
cd ~/Desktop/SwissBunker
git add web
git commit -m "feat(search): FTS5 queries with bm25 ranking and query sanitisation"
```

---

### Task 6: Bundle IIFE con wasm inlineato

**Files:**
- Create: `web/build.mjs`
- Create: `web/src/index.ts`
- Create: `web/test/conformance/reader.html`

**Interfaces:**
- Consumes: tutti i moduli precedenti
- Produces: `web/dist/reader.js`, un singolo script classico caricabile da `file://`, e
  `window.SwissBunkerReader` con `{ openDatabase, FileSource, FtsIndex }`.

**Il vincolo che detta questo task.** Da `file://` l'import di moduli ES e `fetch` di un file
vicino falliscono. Tutto il codice diventa un IIFE e il wasm viaggia in base64 dentro il
bundle, passato all'inizializzazione via `wasmBinary` — la stessa tecnica verificata in P4.

- [ ] **Step 1: Scrivere lo script di build**

```javascript
// web/build.mjs
// Produce a single classic script that runs from file://.
//
// Two things make this necessary, both measured in Phase 0: ES module imports fail under a
// null origin, and so does fetch of a sibling file. So: IIFE format, and the wasm inlined
// as base64 rather than fetched at init.
import { build } from 'esbuild';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';

const WASM = 'node_modules/@sqlite.org/sqlite-wasm/sqlite-wasm/jswasm/sqlite3.wasm';

mkdirSync('src/generated', { recursive: true });
const b64 = readFileSync(WASM).toString('base64');
writeFileSync('src/generated/wasm-binary.ts',
  `// Generated by build.mjs — do not edit.\n` +
  `// The SQLite wasm module, inlined because fetch is unavailable under a null origin.\n` +
  `export const SQLITE_WASM_BASE64 = '${b64}';\n` +
  `export function sqliteWasmBinary(): Uint8Array {\n` +
  `  const raw = atob(SQLITE_WASM_BASE64);\n` +
  `  const out = new Uint8Array(raw.length);\n` +
  `  for (let i = 0; i < raw.length; i++) { out[i] = raw.charCodeAt(i); }\n` +
  `  return out;\n` +
  `}\n`);
console.log(`inlined ${(b64.length / 1024 / 1024).toFixed(2)} MB of base64 wasm`);

const result = await build({
  entryPoints: ['src/index.ts'],
  bundle: true,
  format: 'iife',
  globalName: 'SwissBunkerReader',
  target: ['chrome113', 'firefox147', 'safari26'],
  outfile: 'dist/reader.js',
  minify: process.argv.includes('--minify'),
  sourcemap: false,
  metafile: true
});

const bytes = Object.values(result.metafile.outputs)[0].bytes;
console.log(`built dist/reader.js — ${(bytes / 1024 / 1024).toFixed(2)} MB`);
// The spec budgets the dashboard bundle at under 2 MB, and the wasm alone is 844 KB raw,
// which base64 inflates to ~1.13 MB. Flag it rather than let it drift silently.
if (bytes > 2 * 1024 * 1024) {
  console.warn(`WARNING: over the 2 MB budget from spec §9.1 by ${((bytes - 2*1024*1024)/1024).toFixed(0)} KB`);
}
```

- [ ] **Step 2: Costruire e verificare che non resti nulla da caricare**

```bash
cd ~/Desktop/SwissBunker/web && node build.mjs
# No fetch of a sibling file and no dynamic import may survive in the bundle: either one
# would work over http:// and fail from file://, which is the worst kind of bug to ship.
grep -c "instantiateStreaming" dist/reader.js || echo "  no streaming instantiation: good"
grep -cE "import\(" dist/reader.js || echo "  no dynamic import: good"
ls -lh dist/reader.js
```

- [ ] **Step 3: Scrivere la pagina di conformità**

```html
<!-- web/test/conformance/reader.html -->
<!doctype html>
<meta charset="utf-8">
<title>Reader conformance</title>
<style>body{font:14px/1.6 ui-monospace,monospace;margin:2rem;max-width:80ch}pre{background:#f4f4f5;padding:1rem;border-radius:8px;overflow-x:auto}</style>
<h1>Reader conformance — file://</h1>
<p>Select the volume holding <code>fts-test.sqlite</code>.</p>
<input type="file" id="dir-input" webkitdirectory multiple>
<pre id="probe-output"></pre>
<script src="../../../spikes/phase-0/lib/probe.js"></script>
<script src="../../dist/reader.js"></script>
<script>
// The unit suite runs over http:// because that is where Vitest lives, which makes it the
// CONTROL condition, not the target. These assertions only mean something from file://, so
// they live here and nowhere else.
(function () {
  Probe.init('c1', 'Reader conformance under a null origin');

  document.getElementById('dir-input').addEventListener('change', async function (e) {
    var files = Array.prototype.slice.call(e.target.files);
    var dbFile = files.filter(function (f) { return f.name === 'fts-test.sqlite'; })[0];

    if (!dbFile) {
      await Probe.check('fixture_found', function () { return false; });
      Probe.finish();
      return;
    }
    await Probe.check('fixture_found', function () { return true; });
    Probe.info('database_gb', Math.round(dbFile.size / 1e9 * 100) / 100);

    var R = window.SwissBunkerReader;
    var db = null;

    await Probe.check('opens_multi_gb_database', async function () {
      var t0 = performance.now();
      db = await R.openDatabase(new R.FileSource(dbFile));
      Probe.info('open_ms', Math.round(performance.now() - t0));
      return db !== null;
    });

    // The exit criterion of Phase 1, stated as an assertion rather than a hope.
    await Probe.check('finds_needle_under_500ms', function () {
      if (!db) { return false; }
      var index = new R.FtsIndex(db);
      var t0 = performance.now();
      var hits = index.search('xyzzyneedlemarker');
      var ms = performance.now() - t0;
      Probe.info('search_ms', Math.round(ms * 100) / 100);
      if (hits.length !== 1) { return 'expected 1 hit, got ' + hits.length; }
      if (hits[0].title !== 'Document 1337000') { return 'wrong title: ' + hits[0].title; }
      return ms < 500 ? true : 'found it, but took ' + Math.round(ms) + ' ms';
    });

    // The point of the whole architecture: the index is never fully loaded.
    await Probe.check('never_loaded_whole_file', function () {
      if (!db) { return false; }
      Probe.info('cache_stats', db.stats);
      return db.stats.bytesRead < dbFile.size * 0.05;
    });

    await Probe.check('cache_stayed_bounded', function () {
      return db ? db.stats.cachedBytes <= 128 * 1024 * 1024 : false;
    });

    await Probe.measure('repeat_search_ms', function () {
      var index = new R.FtsIndex(db);
      index.search('xyzzyneedlemarker');
      return Promise.resolve();
    }, 20);

    Probe.finish();
  });
}());
</script>
```

- [ ] **Step 4: Eseguire la suite di conformità**

Riusando il runner della Fase 0, che sa già pilotare `file://` e i picker:

```bash
cd ~/Desktop/SwissBunker/spikes/phase-0
npm install --no-audit --no-fund playwright@1.49.1 && npx playwright install chromium
hdiutil attach ~/swissbunker-fixtures/exfat-test.sparseimage
node tools/run-probes.mjs --engines chromium --probes c1 --only-file --timeout 300
```

Va aggiunta a `PROBES` in `run-probes.mjs` la riga:
`c1: { file: '../../web/test/conformance/reader.html', input: FIXTURES }`

- [ ] **Step 5: Commit**

```bash
cd ~/Desktop/SwissBunker
git add web spikes/phase-0/tools/run-probes.mjs
git commit -m "build(reader): IIFE bundle with inlined wasm, plus file:// conformance suite"
```

---

### Task 7: UI minima di ricerca

**Files:**
- Create: `web/src/ui/app.ts`, `web/src/ui/styles.css`
- Create: `web/dist/START.html`

**Interfaces:**
- Consumes: `FileSource`, `openDatabase`, `FtsIndex`
- Produces: la pagina che l'utente apre davvero.

**Restare minimi.** Questa non è la Console della spec §9: è la prova che lo stack funziona
end-to-end sotto le mani di una persona. Una barra di ricerca, una lista di risultati, uno
stato di caricamento onesto. Il design vero è Fase 5.

- [ ] **Step 1: Scrivere `web/dist/START.html`**

```html
<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>SwissBunker</title>
<link rel="stylesheet" href="styles.css">
<div id="app">
  <header>
    <h1>SwissBunker</h1>
    <p id="status">Select your bunker disk to begin.</p>
    <input type="file" id="disk" webkitdirectory multiple>
  </header>
  <main hidden id="search-area">
    <input type="search" id="q" placeholder="Search the bunker…" autocomplete="off">
    <p id="meta"></p>
    <ol id="results"></ol>
  </main>
</div>
<script src="reader.js"></script>
<script src="app.js"></script>
```

- [ ] **Step 2: Scrivere `web/src/ui/app.ts`**

```typescript
// Minimal search UI. Phase 5 replaces this entirely; its only job is to prove the stack
// works under a real person's hands.
import { FileSource } from '../io/source';
import { openDatabase, type ReaderDatabase } from '../sqlite/open';
import { FtsIndex } from '../search/fts';

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

let index: FtsIndex | null = null;
let db: ReaderDatabase | null = null;

$('disk').addEventListener('change', async (e) => {
  const files = Array.from((e.target as HTMLInputElement).files ?? []);
  const dbFile = files.find(f => f.name.endsWith('.sqlite'));

  if (!dbFile) {
    // The empty state has to say what to do, not just report failure — the user may be
    // standing in front of this with no internet to look anything up.
    $('status').textContent =
      'No index found on that disk. Expected a .sqlite file at the top level.';
    return;
  }

  $('status').textContent = `Opening ${dbFile.name} (${(dbFile.size / 1e9).toFixed(1)} GB)…`;
  const t0 = performance.now();
  db = await openDatabase(new FileSource(dbFile));
  index = new FtsIndex(db);
  $('status').textContent =
    `Ready — ${dbFile.name}, opened in ${Math.round(performance.now() - t0)} ms`;
  $('search-area').hidden = false;
  $<HTMLInputElement>('q').focus();
});

let pending = 0;
$('q').addEventListener('input', (e) => {
  const query = (e.target as HTMLInputElement).value.trim();
  // Debounce: every keystroke otherwise costs a full index lookup, and on a USB disk that
  // is real I/O rather than a cheap in-memory scan.
  window.clearTimeout(pending);
  pending = window.setTimeout(() => runSearch(query), 150);
});

function runSearch(query: string): void {
  const results = $('results');
  if (!index || query.length < 2) {
    results.innerHTML = '';
    $('meta').textContent = '';
    return;
  }

  const t0 = performance.now();
  const hits = index.search(query, { limit: 20 });
  const ms = Math.round(performance.now() - t0);

  $('meta').textContent = hits.length
    ? `${hits.length} results in ${ms} ms`
    : `Nothing in the bunker matches “${query}” — ${ms} ms`;

  results.innerHTML = '';
  for (const hit of hits) {
    const li = document.createElement('li');
    const h = document.createElement('h2');
    h.textContent = hit.title;
    const p = document.createElement('p');
    // snippet() returns <mark> tags we generated ourselves, so this is our own markup,
    // not user input — but the title above still goes through textContent.
    p.innerHTML = hit.snippet;
    li.append(h, p);
    results.append(li);
  }
}
```

- [ ] **Step 3: Provarla davvero**

```bash
cd ~/Desktop/SwissBunker/web && node build.mjs
# One bundle, not two: app.ts imports the reader modules directly, so marking them external
# would leave unresolved references at runtime. The wasm is already inlined by build.mjs.
npx esbuild src/ui/app.ts --bundle --format=iife --outfile=dist/app.js \
  --target=chrome113,firefox147,safari26
cp src/ui/styles.css dist/
open dist/START.html
```

`START.html` carica `reader.js` per l'API pubblica e `app.js` per la UI. I due bundle
condividono il codice, quindi il wasm finisce due volte: accettabile in questa fase, da
unificare nel Task 6 della Fase 5 quando la Console prende il posto di questa pagina.

Selezionare `/Volumes/SWISSTEST`, cercare `xyzzyneedlemarker`, e vedere comparire
**Document 1337000**.

- [ ] **Step 4: Commit**

```bash
cd ~/Desktop/SwissBunker
git add web
git commit -m "feat(ui): minimal search interface over the reader"
```

---

### Task 8: Verifica su Wikipedia italiana reale

**Files:**
- Create: `web/tools/build-test-index.py`
- Create: `docs/reports/YYYY-MM-DD-phase-1-verification.md`

**Interfaces:**
- Consumes: tutto quanto sopra
- Produces: la prova che il criterio di uscita della Fase 1 è soddisfatto su contenuto reale.

**`build-test-index.py` è un'impalcatura, non la Forge.** Costruisce un indice usabile da uno
ZIM con SQLite nativo, e basta: niente download manager, niente wizard, niente incrementalità,
niente chunking per tipo di corpus. Tutto quello è Fase 2 e Fase 3. Confondere i due porta a
buttare via lavoro o, peggio, a spedire un'impalcatura.

- [ ] **Step 1: Procurarsi Wikipedia IT**

```bash
mkdir -p ~/swissbunker-fixtures/zim && cd ~/swissbunker-fixtures/zim
# Kiwix filenames carry a date suffix, so read the library listing rather than guessing.
curl -s "https://download.kiwix.org/zim/wikipedia/" \
  | grep -oE 'wikipedia_it_all_nopic_[0-9-]+\.zim' | sort -u | tail -1
# Then download the name that command printed:
# curl -fL --retry 3 -O "https://download.kiwix.org/zim/wikipedia/<that name>"
```

- [ ] **Step 2: Scrivere il costruttore di indice**

```python
# web/tools/build-test-index.py
"""Build an FTS5 index from a ZIM file.

SCAFFOLDING, NOT THE FORGE. It exists so Phase 1 can be verified against real content
before Phase 2 builds the real acquisition and indexing pipeline. No resumability, no
progress reporting, no per-corpus chunking strategy, no incremental rebuild.

Requires: pip install libzim
"""
import sqlite3
import sys
import pathlib
from libzim.reader import Archive

def build(zim_path: str, out_path: str, limit: int | None = None) -> None:
    archive = Archive(zim_path)
    out = pathlib.Path(out_path)
    out.unlink(missing_ok=True)

    con = sqlite3.connect(out)
    con.execute("PRAGMA journal_mode=OFF")
    con.execute("PRAGMA synchronous=OFF")
    con.execute("PRAGMA page_size=4096")
    # NOT contentless: the index keeps its own copy of the body. That costs space — the text
    # is stored twice, once here and once in the ZIM — but it is what lets snippet() generate
    # highlighted excerpts without a ZIM reader, so Phase 1 can be verified end to end without
    # first building one.
    #
    # Phase 2 should revisit this. A contentless table (`content=''`) roughly halves the index
    # but makes snippets impossible without reading the source document, which is the trade
    # that decides whether the ZIM reader is on the critical path.
    con.execute("CREATE VIRTUAL TABLE docs USING fts5(title, body, zim_path UNINDEXED)")

    total = archive.article_count if limit is None else min(limit, archive.article_count)
    batch, written = [], 0

    for i in range(total):
        try:
            entry = archive._get_entry_by_id(i)
            item = entry.get_item()
            if not item.mimetype.startswith('text/html'):
                continue
            body = bytes(item.content).decode('utf-8', errors='replace')
            batch.append((entry.title, body, entry.path))
        except Exception:
            # A handful of malformed entries in a six-million-article archive must not stop
            # the build. They are counted, not silently dropped.
            continue

        if len(batch) >= 2000:
            con.executemany("INSERT INTO docs(title, body, zim_path) VALUES (?,?,?)", batch)
            written += len(batch)
            batch.clear()
            con.commit()
            print(f"  {written:,} / {total:,}", flush=True)

    if batch:
        con.executemany("INSERT INTO docs(title, body, zim_path) VALUES (?,?,?)", batch)
        written += len(batch)
    con.commit()
    con.execute("INSERT INTO docs(docs) VALUES('optimize')")
    con.commit()
    con.close()
    print(f"built {out} — {written:,} articles, {out.stat().st_size / 1e9:.2f} GB")

if __name__ == '__main__':
    if len(sys.argv) < 3:
        print(__doc__)
        print("usage: build-test-index.py <in.zim> <out.sqlite> [limit]")
        sys.exit(1)
    build(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else None)
```

- [ ] **Step 3: Costruire l'indice sul disco esterno**

```bash
pip install libzim
cd ~/Desktop/SwissBunker/web
# Build on internal disk first: writing a SQLite B-tree straight onto exFAT over USB is
# pathologically slow because of the random write pattern.
python3 tools/build-test-index.py ~/swissbunker-fixtures/zim/wikipedia_it_*.zim \
  ~/swissbunker-fixtures/itwiki-fts.sqlite
cp ~/swissbunker-fixtures/itwiki-fts.sqlite /Volumes/SWISSTEST/
```

- [ ] **Step 4: Misurare contro il criterio di uscita**

Estendere `reader.html` con un set di venti query italiane reali — `terremoto`,
`fotosintesi clorofilliana`, `battaglia di Lepanto`, `come si potano gli ulivi` — e
registrare p50 e p95. **Il criterio è p95 sotto i 500 ms.**

Eseguire due volte: dall'immagine exFAT e da un **disco USB reale**. Solo i numeri USB
contano per il budget.

- [ ] **Step 5: Scrivere il report di verifica**

`docs/reports/<data>-phase-1-verification.md`, con la stessa disciplina della Fase 0: numeri
misurati, criterio soddisfatto o no, e una sezione esplicita su ciò che **non** è stato
dimostrato.

- [ ] **Step 6: Commit e tag**

```bash
cd ~/Desktop/SwissBunker
git add -A
git commit -m "docs(phase-1): verification against real Italian Wikipedia"
git tag -a phase-1-complete -m "Phase 1: the reader works on real content"
git push origin main --tags
```

---

## Criteri di uscita della Fase 1

1. Una pagina aperta da `file://` apre un indice SQLite **multi-gigabyte** su disco esterno e
   restituisce risultati corretti.
2. Ricerca su Wikipedia IT reale: **p95 sotto i 500 ms**, misurato su **disco USB reale**.
3. `bytesRead` resta sotto il 5% della dimensione dell'indice — prova che non viene caricato
   per intero.
4. La cache resta sotto il tetto configurato, con eviction dimostrata.
5. Tutti i test unitari passano in browser mode; la suite di conformità passa da `file://` su
   Chromium, Chrome e almeno un motore non-Chromium.
6. Il bundle è un singolo script classico, senza `import()` né `instantiateStreaming`.
7. Il report di verifica esiste e dichiara i propri limiti.

## Cosa NON fa questa fase

- Nessuna ricerca vettoriale, nessun IVF, nessun embedding. Fase 3.
- Nessun LLM. Fase 4.
- Nessun download, nessun wizard, nessun daemon. Fase 2.
- Nessun design. La UI è funzionale e brutta apposta; la Console è Fase 5.
- Nessuna scrittura sul bunker. Il VFS è di sola lettura per scelta.
