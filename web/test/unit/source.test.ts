import { describe, it, expect } from 'vitest';
import { BufferSource, FileSource } from '../../src/io/source';

// Pages carry their own index, so a read can be checked for CONTENT, not just for length.
// Phase 0 established that silently wrong bytes are the failure mode that actually matters:
// an error is loud, a corrupt index is not.
function makePages(count: number, pageSize = 4096): Uint8Array {
  const buf = new Uint8Array(count * pageSize);
  const enc = new TextEncoder();
  for (let i = 0; i < count; i++) {
    buf.set(enc.encode(`PAGE:${String(i).padStart(12, '0')}:`), i * pageSize);
  }
  return buf;
}

const HEADER_LEN = 'PAGE:000000000000:'.length;

describe('BufferSource', () => {
  it('reads the exact bytes at an arbitrary offset', async () => {
    const src = new BufferSource(makePages(16), 'test.bin');
    const got = await src.read(3 * 4096, HEADER_LEN);
    expect(new TextDecoder().decode(got)).toBe('PAGE:000000000003:');
  });

  it('reports its size and name', () => {
    const src = new BufferSource(makePages(16), 'test.bin');
    expect(src.size).toBe(16 * 4096);
    expect(src.name).toBe('test.bin');
  });

  it('clamps a read running past the end instead of throwing', async () => {
    const src = new BufferSource(makePages(2), 'test.bin');
    const got = await src.read(2 * 4096 - 10, 100);
    expect(got.length).toBe(10);
  });

  it('returns an empty array for a zero-length read', () => {
    const src = new BufferSource(makePages(2), 'test.bin');
    expect(src.readSync(0, 0)!.length).toBe(0);
  });
});

describe('FileSource', () => {
  it('reads the exact bytes synchronously', () => {
    const src = new FileSource(new File([makePages(16)], 'test.bin'));
    const got = src.readSync(3 * 4096, HEADER_LEN);
    expect(got).not.toBeNull();
    expect(new TextDecoder().decode(got!)).toBe('PAGE:000000000003:');
  });

  it('reads the exact bytes asynchronously', async () => {
    const src = new FileSource(new File([makePages(16)], 'test.bin'));
    const got = await src.read(7 * 4096, HEADER_LEN);
    expect(new TextDecoder().decode(got)).toBe('PAGE:000000000007:');
  });

  it('agrees with itself across the sync and async paths', async () => {
    const src = new FileSource(new File([makePages(32)], 'test.bin'));
    for (const page of [0, 1, 17, 31]) {
      const a = src.readSync(page * 4096, 64);
      const b = await src.read(page * 4096, 64);
      expect(Array.from(a!)).toEqual(Array.from(b));
    }
  });

  // Binary safety is the subtle one: readSync moves bytes through a text encoding, and a
  // naive implementation mangles anything above 0x7F. An FTS5 index is full of such bytes.
  it('survives every byte value, including 0x00 and 0xFF', () => {
    const all = new Uint8Array(256);
    for (let i = 0; i < 256; i++) { all[i] = i; }
    const src = new FileSource(new File([all], 'bytes.bin'));
    const got = src.readSync(0, 256);
    expect(Array.from(got!)).toEqual(Array.from(all));
  });

  it('clamps a read running past the end', () => {
    const src = new FileSource(new File([makePages(2)], 'test.bin'));
    expect(src.readSync(2 * 4096 - 10, 100)!.length).toBe(10);
  });
});
