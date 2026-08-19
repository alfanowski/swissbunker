import type { ReaderDatabase } from '../sqlite/open';

export interface SearchHit {
  docId: number;
  title: string;
  snippet: string;
  /** Higher is better. bm25 is negated so the number reads the right way up. */
  score: number;
}

export interface SearchOptions {
  limit?: number;
  offset?: number;
}

/**
 * Turn free user input into a valid FTS5 MATCH expression.
 *
 * FTS5 has its own grammar — quoted phrases, NEAR, OR, prefix stars — and an unbalanced
 * quote is a syntax ERROR, not an empty result set. Someone with no internet who typed a
 * stray apostrophe must still get results, so the parser can never be allowed to throw.
 *
 * The policy is LITERAL WITH PHRASES, chosen deliberately over passing input through to
 * FTS5 and catching failures: whoever searches "come depurare l'acqua" does not know FTS5
 * syntax, and in a bunker a syntax error in front of someone who cannot look anything up is
 * worse than a search that is less powerful. Balanced "quoted phrases" survive as phrases;
 * everything else is escaped to a literal term.
 */
export function sanitiseQuery(raw: string): string {
  const phrases: string[] = [];

  // Pull out balanced quoted phrases first, so what remains can be tokenised safely. An
  // unbalanced quote simply never matches here and falls through to the literal path.
  const rest = raw.replace(/"([^"]+)"/g, (_match, inner: string) => {
    phrases.push(`"${inner.replace(/"/g, '""')}"`);
    return ' ';
  });

  const tokens = rest
    .split(/\s+/)
    // Keep letters (any script, so accents and non-Latin survive), digits, underscore and a
    // trailing star. Everything else — quotes, parentheses, colons — would be FTS5 syntax.
    .map(t => t.replace(/[^\p{L}\p{N}_*]/gu, ''))
    .filter(t => t.length > 0 && t !== '*')
    .map(t => (t.endsWith('*') ? `"${t.replace(/\*+$/, '')}"*` : `"${t}"`));

  const all = [...phrases, ...tokens];
  // An empty MATCH is itself a syntax error, so empty input becomes a term that matches
  // nothing. Returning zero results is correct; throwing is not.
  return all.length > 0 ? all.join(' ') : '""';
}

export class FtsIndex {
  constructor(private readonly db: ReaderDatabase, private readonly table = 'docs') {}

  search(query: string, opts: SearchOptions = {}): SearchHit[] {
    const match = sanitiseQuery(query);
    const limit = opts.limit ?? 20;
    const offset = opts.offset ?? 0;

    // bm25() returns a NEGATIVE score where more negative means a better match, so ordering
    // ascending puts the best hit first. Negating it afterwards gives callers a number that
    // behaves the way a score is expected to.
    const rows = this.db.query<[number, string, string, number]>(
      `SELECT rowid,
              title,
              snippet(${this.table}, 1, '<mark>', '</mark>', '…', 24),
              bm25(${this.table})
         FROM ${this.table}
        WHERE ${this.table} MATCH ?
        ORDER BY bm25(${this.table})
        LIMIT ? OFFSET ?`,
      [match, limit, offset]
    );

    return rows.map(([docId, title, snippet, score]) => ({
      docId, title, snippet, score: -score
    }));
  }

  count(query: string): number {
    const rows = this.db.query<[number]>(
      `SELECT count(*) FROM ${this.table} WHERE ${this.table} MATCH ?`,
      [sanitiseQuery(query)]
    );
    return rows[0]?.[0] ?? 0;
  }
}
