import type { ByteSource } from './source';

export interface PageCacheOptions {
  /**
   * Phase 0 measured 39.7 MB read per query with 1 MB pages: at that size a single 4 KB
   * database page drags a megabyte behind it. 64 KB keeps read-ahead useful — SQLite's
   * access pattern is clustered, so a neighbouring page is often wanted next — while cutting
   * amplification by more than an order of magnitude.
   */
  pageSize?: number;
  /** Hard ceiling. Phase 0's reader had none and grew to 795 MB (risk R13). */
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

/**
 * Page-granular cache over a ByteSource, with LRU eviction.
 *
 * Every read is synchronous, because SQLite's VFS cannot await. The source decides whether
 * that is possible at all; this class only decides what is worth keeping.
 */
export class PageCache {
  private readonly pageSize: number;
  private readonly maxBytes: number;
  // A Map iterates in insertion order, which makes LRU nearly free: delete-then-set moves a
  // key to the end, so the eviction victim is always the first key. No linked list needed.
  private readonly pages = new Map<number, Uint8Array>();
  private readonly counters = {
    hits: 0, misses: 0, evictions: 0, bytesRead: 0, sourceReads: 0
  };

  constructor(private readonly source: ByteSource, opts: PageCacheOptions = {}) {
    this.pageSize = opts.pageSize ?? 65536;
    this.maxBytes = opts.maxBytes ?? 128 * 1024 * 1024;
  }

  get stats(): CacheStats {
    let cachedBytes = 0;
    for (const page of this.pages.values()) { cachedBytes += page.length; }
    return { ...this.counters, cachedBytes };
  }

  clear(): void {
    this.pages.clear();
  }

  /** Fetch a cached page and mark it most-recently-used. */
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
    this.evict();
    return bytes;
  }

  private evict(): void {
    let total = 0;
    for (const page of this.pages.values()) { total += page.length; }
    // Never evict down to nothing: a single page larger than the ceiling must still be
    // usable, or a large-page configuration would deadlock into re-reading forever.
    while (total > this.maxBytes && this.pages.size > 1) {
      const oldest = this.pages.keys().next().value as number;
      const victim = this.pages.get(oldest)!;
      this.pages.delete(oldest);
      total -= victim.length;
      this.counters.evictions++;
    }
  }

  /**
   * Blocking read of any range, assembled from cached pages.
   * Returns null only when the underlying source cannot serve a blocking read.
   */
  readSync(offset: number, length: number): Uint8Array | null {
    if (length <= 0) { return new Uint8Array(0); }
    if (offset >= this.source.size) { return new Uint8Array(0); }

    const wanted = Math.min(length, this.source.size - offset);
    const first = Math.floor(offset / this.pageSize);
    const last = Math.floor((offset + wanted - 1) / this.pageSize);

    // The common case is a read inside one page: return a view, no copy.
    if (first === last) {
      const cached = this.touch(first);
      if (cached) { this.counters.hits++; } else { this.counters.misses++; }
      const page = cached ?? this.load(first);
      if (page === null) { return null; }
      const from = offset - first * this.pageSize;
      return page.subarray(from, Math.min(from + wanted, page.length));
    }

    const out = new Uint8Array(wanted);
    let written = 0;
    for (let i = first; i <= last; i++) {
      const cached = this.touch(i);
      if (cached) { this.counters.hits++; } else { this.counters.misses++; }
      const page = cached ?? this.load(i);
      if (page === null) { return null; }

      const pageStart = i * this.pageSize;
      const from = Math.max(0, offset - pageStart);
      const take = Math.min(page.length - from, wanted - written);
      if (take <= 0) { break; }
      out.set(page.subarray(from, from + take), written);
      written += take;
    }
    return written === wanted ? out : out.subarray(0, written);
  }
}
