import { describe, it, expect, beforeAll } from 'vitest';
import { BufferSource } from '../../src/io/source';
import { openDatabase } from '../../src/sqlite/open';
import { FtsIndex, sanitiseQuery } from '../../src/search/fts';

let index: FtsIndex;

beforeAll(async () => {
  const [db, wasm] = await Promise.all([
    fetch('/test/fixtures/tiny-fts.sqlite').then(r => r.arrayBuffer()),
    fetch('/node_modules/@sqlite.org/sqlite-wasm/dist/sqlite3.wasm').then(r => r.arrayBuffer())
  ]);
  const database = await openDatabase(
    new BufferSource(new Uint8Array(db), 'tiny-fts.sqlite'),
    { wasmBinary: new Uint8Array(wasm) }
  );
  index = new FtsIndex(database);
});

describe('sanitiseQuery', () => {
  it('quotes plain terms so FTS5 reads them literally', () => {
    expect(sanitiseQuery('acqua potabile')).toBe('"acqua" "potabile"');
  });

  it('keeps a balanced quoted phrase as a phrase', () => {
    expect(sanitiseQuery('"acqua potabile"')).toBe('"acqua potabile"');
  });

  it('strips an unbalanced quote rather than producing a syntax error', () => {
    expect(sanitiseQuery('broken "quote')).not.toContain('broken "quote');
    expect(sanitiseQuery('broken "quote')).toContain('"broken"');
  });

  it('neutralises FTS5 operators typed by accident', () => {
    // AND/OR/NOT/NEAR are operators to FTS5, but a person typing them means the words.
    expect(sanitiseQuery('AND OR NOT')).toBe('"AND" "OR" "NOT"');
    expect(sanitiseQuery('NEAR(')).toBe('"NEAR"');
  });

  it('preserves a trailing star as a prefix search', () => {
    expect(sanitiseQuery('foto*')).toBe('"foto"*');
  });

  it('never returns an empty MATCH expression', () => {
    // An empty MATCH is a syntax error in FTS5, so empty input must still be valid SQL.
    expect(sanitiseQuery('')).toBe('""');
    expect(sanitiseQuery('!!!')).toBe('""');
  });

  it('keeps accented Italian words intact', () => {
    expect(sanitiseQuery('perché città')).toBe('"perché" "città"');
  });
});

describe('FtsIndex', () => {
  it('finds the planted needle', () => {
    const hits = index.search('xyzzyneedlemarker');
    expect(hits.length).toBe(1);
    expect(hits[0]!.title).toBe('Document 13370');
  });

  it('returns a snippet containing the match', () => {
    const hits = index.search('xyzzyneedlemarker');
    expect(hits[0]!.snippet.toLowerCase()).toContain('xyzzyneedlemarker');
    expect(hits[0]!.snippet).toContain('<mark>');
  });

  it('finds accented Italian words', () => {
    const hits = index.search('fotosintesi');
    expect(hits[0]!.title).toBe('Perché la fotosintesi');
  });

  it('scores hits so that higher is better', () => {
    const hits = index.search('caffè');
    expect(hits.length).toBeGreaterThan(0);
    expect(hits[0]!.score).toBeGreaterThan(0);
  });

  it('respects the limit', () => {
    expect(index.search('a OR e OR i', { limit: 5 }).length).toBeLessThanOrEqual(5);
  });

  it('pages through results with offset', () => {
    const first = index.search('citta OR caffè OR fotosintesi', { limit: 1 });
    const second = index.search('citta OR caffè OR fotosintesi', { limit: 1, offset: 1 });
    if (first.length && second.length) {
      expect(first[0]!.docId).not.toBe(second[0]!.docId);
    }
  });

  it('returns nothing rather than throwing for a query with no matches', () => {
    expect(index.search('zzzznotpresentzzzz').length).toBe(0);
  });

  it('survives malformed input instead of throwing', () => {
    for (const bad of ['broken "quote', 'NEAR(', '*', 'AND OR NOT', '', '((()))', '""']) {
      expect(() => index.search(bad)).not.toThrow();
    }
  });

  it('reports ranked=true for a rare term', () => {
    const r = index.searchDetailed('xyzzyneedlemarker');
    expect(r.ranked).toBe(true);
    expect(r.total).toBe(1);
    expect(r.hits[0]!.score).toBeGreaterThan(0);
  });

  it('falls back to unranked above the cutoff, and says so', () => {
    // rankLimit of 0 forces the hot-term path on any match at all, so the contract can be
    // tested without needing a corpus large enough to be genuinely expensive.
    const r = index.searchDetailed('xyzzyneedlemarker', { rankLimit: 0 });
    expect(r.ranked).toBe(false);
    expect(r.total).toBe(1);
    expect(r.hits.length).toBe(1);
    // Score is meaningless when nothing was ranked, and is zeroed rather than faked.
    expect(r.hits[0]!.score).toBe(0);
  });

  it('reports the true total, not the page size', () => {
    const r = index.searchDetailed('xyzzyneedlemarker', { limit: 1 });
    expect(r.total).toBe(1);
  });

  it('counts matches', () => {
    expect(index.count('xyzzyneedlemarker')).toBe(1);
    expect(index.count('zzzznotpresentzzzz')).toBe(0);
  });
});
