// Public surface of the reader bundle.
//
// Exposed on window.SwissBunkerReader by build.mjs. Kept deliberately small: everything
// here becomes API that the Console and the conformance suite depend on.
export { FileSource, BufferSource, type ByteSource } from './io/source';
export { PageCache, type CacheStats } from './io/page-cache';
export { openDatabase, type ReaderDatabase, type OpenOptions } from './sqlite/open';
export { FtsIndex, sanitiseQuery, type SearchHit } from './search/fts';
export { sqliteWasmBinary } from './generated/wasm-binary';
