// Minimal read-only VFS on @sqlite.org/sqlite-wasm.
//
// Same binary criterion as the wa-sqlite spike: open and query a real database with EVERY
// read going through this VFS, counted.
//
// This library exposes the C API rather than a JS base class, so the VFS is built by filling
// two structs — sqlite3_vfs and sqlite3_io_methods — and handing them to sqlite3.vfs.installVfs.
// Members and helper names were read from the live objects, not guessed.
//
// Reads are synchronous via fs.readSync here. In the browser the same call becomes
// FileSource.readSync (synchronous XHR over a Blob URL, 0.7 ms measured in Phase 0).
import sqlite3InitModule from '@sqlite.org/sqlite-wasm';
import { openSync, readSync, fstatSync, closeSync } from 'node:fs';

const DB_PATH = 'tiny-fts.sqlite';
export const stats = { xOpen: 0, xRead: 0, xFileSize: 0, xAccess: 0, bytesRead: 0 };

const sqlite3 = await sqlite3InitModule();
const { capi, wasm } = sqlite3;

// One open file is enough for a spike. Production keys this by the sqlite3_file pointer.
let fd = null;
let fileSize = 0;

const ioMethods = new capi.sqlite3_io_methods();
const vfs = new capi.sqlite3_vfs();

// Inherit everything we do not implement from the default VFS: xRandomness, xSleep,
// xCurrentTime and the dl* family are all fine as-is, and reimplementing them would be
// pure risk for no benefit.
const defaultVfs = new capi.sqlite3_vfs(capi.sqlite3_vfs_find(null));
vfs.$iVersion = 2;
vfs.$szOsFile = capi.sqlite3_file.structInfo.sizeof;
vfs.$mxPathname = 1024;
vfs.$xRandomness = defaultVfs.$xRandomness;
vfs.$xSleep = defaultVfs.$xSleep;
vfs.$xCurrentTime = defaultVfs.$xCurrentTime;
vfs.$xCurrentTimeInt64 = defaultVfs.$xCurrentTimeInt64;

const vfsMethods = {
  xOpen(_pVfs, _zName, pFile, _flags, pOutFlags) {
    stats.xOpen++;
    fd = openSync(DB_PATH, 'r');
    fileSize = fstatSync(fd).size;
    // Attach our io_methods to this file handle: this is what routes every later read here.
    const f = new capi.sqlite3_file(pFile);
    f.$pMethods = ioMethods.pointer;
    f.dispose();
    // Report READONLY so SQLite never tries to create a journal.
    wasm.poke(pOutFlags, capi.SQLITE_OPEN_READONLY, 'i32');
    return 0;
  },
  xAccess(_pVfs, _zName, _flags, pResOut) {
    stats.xAccess++;
    wasm.poke(pResOut, 1, 'i32');
    return 0;
  },
  xDelete() { return capi.SQLITE_READONLY; },
  xFullPathname(_pVfs, zName, nOut, pOut) {
    const name = wasm.cstrToJs(zName);
    const bytes = new TextEncoder().encode(name);
    if (bytes.length + 1 > nOut) { return capi.SQLITE_CANTOPEN; }
    wasm.heap8u().set(bytes, pOut);
    wasm.poke(pOut + bytes.length, 0, 'i8');
    return 0;
  },
  xGetLastError(_pVfs, _n, _pOut) { return 0; }
};

const ioMethodsImpl = {
  xClose() {
    if (fd !== null) { closeSync(fd); fd = null; }
    return 0;
  },
  xRead(_pFile, pDest, n, offset64) {
    stats.xRead++;
    const buf = Buffer.allocUnsafe(n);
    const got = readSync(fd, buf, 0, n, Number(offset64));
    stats.bytesRead += got;
    wasm.heap8u().set(buf.subarray(0, got), pDest);
    if (got < n) {
      // SQLite wants the tail zeroed and SHORT_READ returned; it relies on this when
      // reading past the end of a partial page and treats it as normal, not an error.
      wasm.heap8u().fill(0, pDest + got, pDest + n);
      return capi.SQLITE_IOERR_SHORT_READ;
    }
    return 0;
  },
  xFileSize(_pFile, pSize64) {
    stats.xFileSize++;
    wasm.poke(pSize64, BigInt(fileSize), 'i64');
    return 0;
  },
  // Read-only: no journal, no write path, no locking protocol — most of what makes a VFS
  // hard simply does not apply to an immutable bunker.
  xWrite() { return capi.SQLITE_READONLY; },
  xTruncate() { return capi.SQLITE_READONLY; },
  xSync() { return 0; },
  xLock() { return 0; },
  xUnlock() { return 0; },
  xCheckReservedLock(_pFile, pResOut) { wasm.poke(pResOut, 0, 'i32'); return 0; },
  xFileControl() { return capi.SQLITE_NOTFOUND; },
  xSectorSize() { return 4096; },
  xDeviceCharacteristics() { return capi.SQLITE_IOCAP_IMMUTABLE; }
};

ioMethods.$iVersion = 1;
sqlite3.vfs.installVfs({
  io:  { struct: ioMethods, methods: ioMethodsImpl },
  vfs: { struct: vfs, methods: vfsMethods, name: 'bunker', asDefault: false }
});

let ok = false, rows = [], error = null;
try {
  const db = new sqlite3.oo1.DB({ filename: DB_PATH, flags: 'r', vfs: 'bunker' });
  db.exec({
    sql: "SELECT title FROM docs WHERE docs MATCH 'xyzzyneedlemarker'",
    rowMode: 'array',
    callback: (r) => rows.push(r[0])
  });
  db.close();
  ok = rows.length === 1 && rows[0] === 'Document 13370';
} catch (e) {
  error = e.message;
}

console.log(JSON.stringify({
  library: '@sqlite.org/sqlite-wasm',
  needleFound: ok,
  rows,
  error,
  stats,
  readsWentThroughVfs: stats.xRead > 0,
  fractionOfFileRead: fileSize ? +(stats.bytesRead / fileSize).toFixed(3) : null
}, null, 2));
process.exit(ok ? 0 : 1);
