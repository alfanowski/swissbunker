import { describe, it, expect, beforeAll } from 'vitest';
import { BufferSource } from '../../src/io/source';
import { openDatabase, type ReaderDatabase } from '../../src/sqlite/open';

// A real SQLite file, built by tools/make-tiny-fixture.py. Fetched rather than imported:
// Vitest serves the project root over http, and this is the control condition anyway — the
// file:// path is exercised by the conformance suite.
let dbBytes: Uint8Array;
let wasmBinary: Uint8Array;

beforeAll(async () => {
  const [db, wasm] = await Promise.all([
    fetch('/test/fixtures/tiny-fts.sqlite').then(r => r.arrayBuffer()),
    // Load the wasm the same way production will — as bytes, not as a fetch the library
    // performs for itself. Under file:// that fetch is blocked, so testing the other path
    // would test something the product never does.
    fetch('/node_modules/@sqlite.org/sqlite-wasm/dist/sqlite3.wasm').then(r => r.arrayBuffer())
  ]);
  dbBytes = new Uint8Array(db);
  wasmBinary = new Uint8Array(wasm);
});

async function open(opts = {}): Promise<ReaderDatabase> {
  return openDatabase(new BufferSource(dbBytes, 'tiny-fts.sqlite'), { wasmBinary, ...opts });
}

describe('openDatabase', () => {
  it('opens a database through the custom VFS', async () => {
    const db = await open();
    expect(db).toBeDefined();
    db.close();
  });

  it('reads the schema through the VFS', async () => {
    const db = await open();
    const rows = db.query<[string]>("SELECT name FROM sqlite_master WHERE type='table'");
    expect(rows.flat()).toContain('docs');
    db.close();
  });

  it('answers an FTS5 query with the planted needle', async () => {
    const db = await open();
    const rows = db.query<[string]>(
      "SELECT title FROM docs WHERE docs MATCH 'xyzzyneedlemarker'");
    expect(rows.length).toBe(1);
    expect(rows[0]![0]).toBe('Document 13370');
    db.close();
  });

  it('handles Italian text with accents', async () => {
    const db = await open();
    const rows = db.query<[string]>("SELECT title FROM docs WHERE docs MATCH 'fotosintesi'");
    expect(rows[0]![0]).toBe('Perché la fotosintesi');
    db.close();
  });

  it('binds parameters', async () => {
    const db = await open();
    const rows = db.query<[string]>(
      'SELECT title FROM docs WHERE docs MATCH ?', ['xyzzyneedlemarker']);
    expect(rows[0]![0]).toBe('Document 13370');
    db.close();
  });

  // The point of the entire architecture, stated as an assertion: the file is never loaded.
  it('reads far less than the whole file to answer a query', async () => {
    const db = await open({ pageSize: 4096 });
    db.query("SELECT title FROM docs WHERE docs MATCH 'xyzzyneedlemarker'");
    expect(db.stats.sourceReads).toBeGreaterThan(0);
    expect(db.stats.bytesRead).toBeLessThan(dbBytes.length * 0.25);
    db.close();
  });

  it('keeps the cache under its ceiling', async () => {
    const db = await open({ pageSize: 4096, maxBytes: 64 * 1024 });
    db.query("SELECT title FROM docs WHERE docs MATCH 'xyzzyneedlemarker'");
    expect(db.stats.cachedBytes).toBeLessThanOrEqual(64 * 1024);
    db.close();
  });

  it('supports two databases open at once', async () => {
    const a = await open();
    const b = await open();
    expect(a.query("SELECT title FROM docs WHERE docs MATCH 'xyzzyneedlemarker'")[0]![0])
      .toBe('Document 13370');
    expect(b.query("SELECT title FROM docs WHERE docs MATCH 'fotosintesi'")[0]![0])
      .toBe('Perché la fotosintesi');
    a.close();
    b.close();
  });
});
