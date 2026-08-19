import type { ByteSource } from '../io/source';
import { PageCache, type CacheStats, type PageCacheOptions } from '../io/page-cache';

/**
 * A read-only SQLite VFS backed by a ByteSource.
 *
 * Read-only is not a limitation here but a simplification worth naming: the bunker never
 * mutates in Portable mode, so there is no journal, no write path and no locking protocol —
 * which is most of what makes a VFS hard. Writes return SQLITE_READONLY, locks are no-ops.
 *
 * Every read is synchronous, which is the whole reason FileSource.readSync exists: SQLite's
 * VFS cannot await, File.slice() is asynchronous, and a null origin denies SharedArrayBuffer.
 *
 * The struct members and helper names used below were read off the live sqlite3 object
 * during the Phase 1 Task 1 spike (see web/spike-vfs/), not guessed. That spike proved this
 * design answers an FTS5 query after reading 2.1% of a database.
 */

// Constants, spelled out so this module does not depend on capi being loaded to be read.
export const SQLITE_OK = 0;
export const SQLITE_READONLY = 8;
export const SQLITE_NOTFOUND = 12;
export const SQLITE_CANTOPEN = 14;
export const SQLITE_IOERR_READ = 266;
export const SQLITE_IOERR_SHORT_READ = 522;
export const SQLITE_IOCAP_IMMUTABLE = 0x2000;
export const SQLITE_OPEN_READONLY = 0x00000001;

/** The subset of the sqlite3 API surface this module needs. Kept narrow on purpose. */
interface Sqlite3Api {
  capi: Record<string, any>;
  wasm: {
    poke(ptr: number, value: number | bigint, type: string): void;
    heap8u(): Uint8Array;
    cstrToJs(ptr: number): string;
  };
  vfs: { installVfs(opts: unknown): unknown };
}

export interface VfsHandle {
  readonly name: string;
  readonly stats: CacheStats;
  dispose(): void;
}

/**
 * Register a read-only VFS that serves one file.
 *
 * One VFS per file rather than a registry keyed by filename: the bunker opens a small,
 * known set of indexes, and a per-file VFS keeps the open path free of lookup and lifetime
 * questions that would otherwise need solving for no benefit.
 */
export function registerReadOnlyVfs(
  sqlite3: Sqlite3Api,
  source: ByteSource,
  vfsName: string,
  opts: PageCacheOptions = {}
): VfsHandle {
  const { capi, wasm } = sqlite3;
  const cache = new PageCache(source, opts);

  const ioMethods = new capi.sqlite3_io_methods();
  const vfs = new capi.sqlite3_vfs();

  // Inherit what we do not implement from the default VFS. xRandomness, xSleep and the
  // clock functions are all fine as-is, and reimplementing them would be pure risk.
  const defaultVfs = new capi.sqlite3_vfs(capi.sqlite3_vfs_find(null));
  vfs.$iVersion = 2;
  vfs.$szOsFile = capi.sqlite3_file.structInfo.sizeof;
  vfs.$mxPathname = 1024;
  vfs.$xRandomness = defaultVfs.$xRandomness;
  vfs.$xSleep = defaultVfs.$xSleep;
  vfs.$xCurrentTime = defaultVfs.$xCurrentTime;
  vfs.$xCurrentTimeInt64 = defaultVfs.$xCurrentTimeInt64;
  ioMethods.$iVersion = 1;

  const vfsMethods = {
    xOpen(_pVfs: number, _zName: number, pFile: number, _flags: number, pOutFlags: number) {
      // Attaching our io_methods to this file handle is what routes every later read here.
      const f = new capi.sqlite3_file(pFile);
      f.$pMethods = ioMethods.pointer;
      f.dispose();
      // Report READONLY so SQLite never attempts to create a journal.
      wasm.poke(pOutFlags, SQLITE_OPEN_READONLY, 'i32');
      return SQLITE_OK;
    },
    xAccess(_pVfs: number, _zName: number, _flags: number, pResOut: number) {
      // The single file this VFS serves always exists; nothing else is reachable through it.
      wasm.poke(pResOut, 1, 'i32');
      return SQLITE_OK;
    },
    xDelete() { return SQLITE_READONLY; },
    xFullPathname(_pVfs: number, zName: number, nOut: number, pOut: number) {
      const bytes = new TextEncoder().encode(wasm.cstrToJs(zName));
      if (bytes.length + 1 > nOut) { return SQLITE_CANTOPEN; }
      wasm.heap8u().set(bytes, pOut);
      wasm.poke(pOut + bytes.length, 0, 'i8');
      return SQLITE_OK;
    },
    xGetLastError() { return SQLITE_OK; }
  };

  const ioMethodsImpl = {
    xClose() { return SQLITE_OK; },

    xRead(_pFile: number, pDest: number, n: number, offset64: bigint) {
      const got = cache.readSync(Number(offset64), n);
      if (got === null) { return SQLITE_IOERR_READ; }
      wasm.heap8u().set(got, pDest);
      if (got.length < n) {
        // SQLite wants the tail zeroed and SHORT_READ returned. It relies on this when
        // reading past the end of a partial page and treats it as normal, not an error —
        // returning a hard error here breaks opening perfectly valid databases.
        wasm.heap8u().fill(0, pDest + got.length, pDest + n);
        return SQLITE_IOERR_SHORT_READ;
      }
      return SQLITE_OK;
    },

    xFileSize(_pFile: number, pSize64: number) {
      wasm.poke(pSize64, BigInt(source.size), 'i64');
      return SQLITE_OK;
    },

    xWrite() { return SQLITE_READONLY; },
    xTruncate() { return SQLITE_READONLY; },
    xSync() { return SQLITE_OK; },
    xLock() { return SQLITE_OK; },
    xUnlock() { return SQLITE_OK; },
    xCheckReservedLock(_pFile: number, pResOut: number) {
      wasm.poke(pResOut, 0, 'i32');
      return SQLITE_OK;
    },
    xFileControl() { return SQLITE_NOTFOUND; },
    xSectorSize() { return 4096; },
    // Telling SQLite the file is immutable lets it skip change-counter re-reads on every
    // transaction, which on a USB disk is real I/O saved rather than a micro-optimisation.
    xDeviceCharacteristics() { return SQLITE_IOCAP_IMMUTABLE; }
  };

  sqlite3.vfs.installVfs({
    io:  { struct: ioMethods, methods: ioMethodsImpl },
    vfs: { struct: vfs, methods: vfsMethods, name: vfsName, asDefault: false }
  });

  return {
    name: vfsName,
    get stats() { return cache.stats; },
    dispose() {
      capi.sqlite3_vfs_unregister(vfs.pointer);
      vfs.dispose();
      ioMethods.dispose();
      cache.clear();
    }
  };
}
