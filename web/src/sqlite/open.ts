import sqlite3InitModule from '@sqlite.org/sqlite-wasm';
import type { ByteSource } from '../io/source';
import type { CacheStats } from '../io/page-cache';
import { registerReadOnlyVfs, type VfsHandle } from './vfs';

export interface OpenOptions {
  /** Page size for the cache. Same meaning as PageCacheOptions.pageSize. */
  pageSize?: number;
  /** Cache ceiling in bytes. Same meaning as PageCacheOptions.maxBytes. */
  maxBytes?: number;
  /**
   * The wasm bytes. Required under file://, where fetch of a sibling file is blocked, so the
   * build inlines them. Left undefined the library fetches its own, which works over http://
   * and fails in the condition that matters.
   */
  wasmBinary?: Uint8Array;
}

export interface ReaderDatabase {
  query<T = unknown[]>(sql: string, params?: unknown[]): T[];
  close(): void;
  readonly stats: CacheStats;
}

// The wasm module is initialised once per page: it is 844 KB and stateless across databases,
// so re-initialising per open would waste both time and memory for nothing.
let modulePromise: Promise<any> | null = null;

// The shipped typings declare a no-argument factory, but the implementation is a standard
// Emscripten module factory and does accept the options object — including wasmBinary, which
// is the only way to start it where fetch is unavailable. Verified by reading dist/index.mjs.
type InitFactory = (opts?: { wasmBinary?: Uint8Array; locateFile?: (f: string) => string }) => Promise<any>;
const init = sqlite3InitModule as unknown as InitFactory;

function initSqlite(wasmBinary?: Uint8Array): Promise<any> {
  if (!modulePromise) {
    // Without wasmBinary the library resolves its .wasm relative to import.meta.url and
    // fetches it — which works over http:// and fails under a null origin. Passing the bytes
    // means the production path and the tested path are the same path.
    //
    // locateFile must be supplied as well, and this is not optional. Emscripten resolves the
    // wasm filename before it ever looks at wasmBinary:
    //
    //   function ag(){ return s.locateFile ? MA("sqlite3.wasm") : new URL("sqlite3.wasm", …).href }
    //
    // In an IIFE bundle import.meta.url collapses to something that is not a valid URL, so
    // the else branch throws "Failed to construct 'URL'" and initialisation dies before the
    // inlined bytes are reached. Providing locateFile takes the other branch entirely.
    // Found by the file:// conformance suite; the http:// unit tests never saw it, because
    // over http the URL resolves fine.
    modulePromise = init(
      wasmBinary ? { wasmBinary, locateFile: (f: string) => f } : undefined
    );
  }
  return modulePromise;
}

// VFS names must be unique within the SQLite instance, and a bunker opens several indexes.
let vfsCounter = 0;

/**
 * Open a database that lives behind a ByteSource, reading it lazily.
 *
 * The database is never loaded: SQLite pulls pages through the VFS as its B-tree walk needs
 * them. The Phase 1 Task 1 spike measured an FTS5 query answered after reading 2.1% of the
 * file, and the fraction falls as the file grows, because B-tree depth grows with the
 * logarithm of the row count while the file grows linearly.
 */
export async function openDatabase(
  source: ByteSource,
  opts: OpenOptions = {}
): Promise<ReaderDatabase> {
  const sqlite3 = await initSqlite(opts.wasmBinary);

  const vfsName = `bunker${vfsCounter++}`;
  const handle: VfsHandle = registerReadOnlyVfs(sqlite3, source, vfsName, {
    pageSize: opts.pageSize,
    maxBytes: opts.maxBytes
  });

  const db = new sqlite3.oo1.DB({ filename: source.name, flags: 'r', vfs: vfsName });

  return {
    query<T = unknown[]>(sql: string, params: unknown[] = []): T[] {
      const rows: T[] = [];
      db.exec({
        sql,
        bind: params.length > 0 ? params : undefined,
        rowMode: 'array',
        callback: (row: unknown) => { rows.push(row as T); }
      });
      return rows;
    },
    close(): void {
      db.close();
      handle.dispose();
    },
    get stats(): CacheStats { return handle.stats; }
  };
}
