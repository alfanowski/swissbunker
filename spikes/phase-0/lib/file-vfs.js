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
