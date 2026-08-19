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

const HEADER_LEN = 18;

describe('PageCache correctness', () => {
  it('returns the correct bytes', () => {
    const cache = new PageCache(new BufferSource(pages(64), 'x'), { pageSize: 65536 });
    const got = cache.readSync(5 * 4096, HEADER_LEN);
    expect(new TextDecoder().decode(got!)).toBe('PAGE:000000000005:');
  });

  it('returns the correct bytes for a read spanning two pages', () => {
    const raw = pages(64);
    const cache = new PageCache(new BufferSource(raw, 'x'), { pageSize: 8192 });
    // With 8192-byte pages a read at 8180 crosses the boundary into the next page.
    const got = cache.readSync(8180, 40);
    expect(Array.from(got!)).toEqual(Array.from(raw.subarray(8180, 8220)));
  });

  it('returns the correct bytes for a read spanning many pages', () => {
    const raw = pages(64);
    const cache = new PageCache(new BufferSource(raw, 'x'), { pageSize: 4096 });
    const got = cache.readSync(1000, 20000);
    expect(Array.from(got!)).toEqual(Array.from(raw.subarray(1000, 21000)));
  });

  it('clamps a read running past the end', () => {
    const cache = new PageCache(new BufferSource(pages(2), 'x'), { pageSize: 4096 });
    expect(cache.readSync(2 * 4096 - 10, 100)!.length).toBe(10);
  });
});

describe('PageCache caching', () => {
  it('serves a repeated read without touching the source again', () => {
    const cache = new PageCache(new BufferSource(pages(64), 'x'), { pageSize: 65536 });
    cache.readSync(0, 100);
    const after = cache.stats.sourceReads;
    cache.readSync(50, 100);
    expect(cache.stats.sourceReads).toBe(after);
    expect(cache.stats.hits).toBeGreaterThan(0);
  });

  it('reads far less than the file to answer scattered small reads', () => {
    const cache = new PageCache(new BufferSource(pages(1024), 'x'), { pageSize: 4096 });
    for (let i = 0; i < 20; i++) { cache.readSync(i * 4096, 16); }
    // 20 pages of 4 KB, not the whole 4 MB file.
    expect(cache.stats.bytesRead).toBe(20 * 4096);
  });
});

describe('PageCache eviction', () => {
  it('stays under maxBytes', () => {
    const cache = new PageCache(new BufferSource(pages(512), 'x'),
                                { pageSize: 4096, maxBytes: 40960 });
    for (let i = 0; i < 100; i++) { cache.readSync(i * 4096, 16); }
    expect(cache.stats.cachedBytes).toBeLessThanOrEqual(40960);
    expect(cache.stats.evictions).toBeGreaterThan(0);
  });

  // The distinction that makes this LRU rather than FIFO. Phase 0's reader had no eviction
  // at all and grew to 795 MB (risk R13); FIFO would fix the size but throw away the hot
  // pages an index re-reads constantly.
  it('evicts the least recently USED page, not the oldest inserted', () => {
    const cache = new PageCache(new BufferSource(pages(512), 'x'),
                                { pageSize: 4096, maxBytes: 3 * 4096 });
    cache.readSync(0, 16);           // page 0
    cache.readSync(4096, 16);        // page 1
    cache.readSync(8192, 16);        // page 2
    cache.readSync(0, 16);           // page 0 again -> now the most recently used
    cache.readSync(12288, 16);       // page 3 -> must evict page 1, not page 0

    const before = cache.stats.sourceReads;
    cache.readSync(0, 16);           // page 0 must still be resident
    expect(cache.stats.sourceReads).toBe(before);

    cache.readSync(4096, 16);        // page 1 must have been the victim
    expect(cache.stats.sourceReads).toBe(before + 1);
  });

  it('clear() empties the cache', () => {
    const cache = new PageCache(new BufferSource(pages(64), 'x'), { pageSize: 4096 });
    cache.readSync(0, 16);
    cache.clear();
    expect(cache.stats.cachedBytes).toBe(0);
  });
});
