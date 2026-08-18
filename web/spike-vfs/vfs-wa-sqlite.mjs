// Minimal read-only VFS on wa-sqlite.
//
// The criterion is binary: a database on disk is opened and queried with EVERY read going
// through this VFS, counted. Correctness beyond that is not the point — this measures how
// much code a working VFS costs in each library.
//
// Reads are synchronous here via fs.readSync. In the browser the same call becomes
// FileSource.readSync (synchronous XHR over a Blob URL, 0.7 ms measured in Phase 0). The
// VFS logic is identical either way, which is the whole reason ByteSource exists.
//
// Signatures below were read out of node_modules/wa-sqlite/src/VFS.js, not guessed. Two
// assumptions died on contact with the source: VFS.Base has no constructor and no `name`
// (the subclass sets it), and the out-parameters arrive as DataView already — there is no
// this.dataView() helper.
import * as SQLite from 'wa-sqlite';
import SQLiteESMFactory from 'wa-sqlite/dist/wa-sqlite.mjs';
import * as VFS from 'wa-sqlite/src/VFS.js';
import { openSync, readSync, fstatSync, closeSync, readFileSync } from 'node:fs';

export const stats = { xOpen: 0, xRead: 0, xFileSize: 0, bytesRead: 0 };

class ReadOnlyFileVFS extends VFS.Base {
  name = 'bunker';
  #fd = null;
  #size = 0;

  constructor(path) {
    super();
    this.path = path;
  }

  xOpen(_name, _fileId, _flags, pOutFlags) {
    stats.xOpen++;
    this.#fd = openSync(this.path, 'r');
    this.#size = fstatSync(this.#fd).size;
    // Report READONLY back so SQLite never attempts to create a journal.
    pOutFlags.setInt32(0, SQLite.SQLITE_OPEN_READONLY, true);
    return VFS.SQLITE_OK;
  }

  xClose() {
    if (this.#fd !== null) { closeSync(this.#fd); this.#fd = null; }
    return VFS.SQLITE_OK;
  }

  xRead(_fileId, pData, iOffset) {
    stats.xRead++;
    const n = readSync(this.#fd, pData, 0, pData.byteLength, iOffset);
    stats.bytesRead += n;
    if (n < pData.byteLength) {
      // SQLite wants the tail zeroed and SHORT_READ returned. It relies on this when reading
      // past the end of a partial page and treats it as normal, not as an error.
      pData.fill(0, n);
      return VFS.SQLITE_IOERR_SHORT_READ;
    }
    return VFS.SQLITE_OK;
  }

  xFileSize(_fileId, pSize64) {
    stats.xFileSize++;
    pSize64.setBigInt64(0, BigInt(this.#size), true);
    return VFS.SQLITE_OK;
  }

  // Read-only: the bunker never mutates in Portable mode, which removes the journal, the
  // write path and the locking protocol — most of what makes a VFS hard.
  xWrite() { return VFS.SQLITE_READONLY; }
  xTruncate() { return VFS.SQLITE_READONLY; }
  xSync() { return VFS.SQLITE_OK; }
  xLock() { return VFS.SQLITE_OK; }
  xUnlock() { return VFS.SQLITE_OK; }
  xCheckReservedLock(_fileId, pResOut) { pResOut.setInt32(0, 0, true); return VFS.SQLITE_OK; }
  xSectorSize() { return 4096; }
  xDeviceCharacteristics() { return 0x2000; }         // SQLITE_IOCAP_IMMUTABLE
  xAccess(_name, _flags, pResOut) { pResOut.setInt32(0, 1, true); return VFS.SQLITE_OK; }
  xDelete() { return VFS.SQLITE_OK; }
}

// wa-sqlite fetches its own .wasm at init, which fails in Node on a file path and would fail
// identically under a null origin. Handing over the bytes skips the fetch entirely — the same
// wasmBinary workaround Phase 0 needed for sql.js.
const module = await SQLiteESMFactory({
  wasmBinary: readFileSync('node_modules/wa-sqlite/dist/wa-sqlite.wasm')
});
const sqlite3 = SQLite.Factory(module);
sqlite3.vfs_register(new ReadOnlyFileVFS('tiny-fts.sqlite'), false);

const db = await sqlite3.open_v2('tiny-fts.sqlite', SQLite.SQLITE_OPEN_READONLY, 'bunker');
const rows = [];
await sqlite3.exec(db, "SELECT title FROM docs WHERE docs MATCH 'xyzzyneedlemarker'",
                   (row) => rows.push(row[0]));
await sqlite3.close(db);

const fileSize = fstatSync(openSync('tiny-fts.sqlite', 'r')).size;
const ok = rows.length === 1 && rows[0] === 'Document 13370';
console.log(JSON.stringify({
  library: 'wa-sqlite',
  needleFound: ok,
  rows,
  stats,
  readsWentThroughVfs: stats.xRead > 0,
  fractionOfFileRead: +(stats.bytesRead / fileSize).toFixed(3)
}, null, 2));
process.exit(ok ? 0 : 1);
