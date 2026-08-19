import type { ReaderDatabase } from '../sqlite/open';

export interface SearchHit {
  docId: number;
  title: string;
  snippet: string;
  /** Higher is better. bm25 is negated so the number reads the right way up. Zero when the
   *  result set was too large to rank — see SearchResult.ranked. */
  score: number;
}

export interface SearchResult {
  hits: SearchHit[];
  /** Total documents matching, which can be far larger than hits.length. */
  total: number;
  /**
   * False when the term was too common to rank and results are in storage order.
   *
   * This is surfaced rather than hidden because it changes what the results MEAN: unranked
   * hits are "twenty documents containing this word", not "the twenty best". A UI that
   * presents the two identically is lying to someone who cannot check.
   */
  ranked: boolean;
}

export interface SearchOptions {
  limit?: number;
  offset?: number;
  /**
   * Above this many matches, ranking is skipped.
   *
   * Measured on a 6.22 GB index over file://: ranking a term matching 35,575 documents cost
   * 2120 ms and 5984 reads, against 12.9 ms and 39 reads unranked — 164x. ORDER BY rank was
   * no better than ORDER BY bm25(): FTS5 must walk the whole doclist to score it, whichever
   * syntax is used, so this is a design limit rather than a query-writing mistake.
   *
   * count(*) costs about 41 reads, so the decision itself is nearly free.
   */
  rankLimit?: number;
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

/** Default cutoff. Chosen from the measurement documented on SearchOptions.rankLimit. */
export const DEFAULT_RANK_LIMIT = 2000;

export class FtsIndex {
  constructor(private readonly db: ReaderDatabase, private readonly table = 'docs') {}

  /** Convenience wrapper for callers that only want the hits. */
  search(query: string, opts: SearchOptions = {}): SearchHit[] {
    return this.searchDetailed(query, opts).hits;
  }

  searchDetailed(query: string, opts: SearchOptions = {}): SearchResult {
    const match = sanitiseQuery(query);
    const limit = opts.limit ?? 20;
    const offset = opts.offset ?? 0;
    const rankLimit = opts.rankLimit ?? DEFAULT_RANK_LIMIT;
    const snip = `snippet(${this.table}, 1, '<mark>', '</mark>', '…', 24)`;

    // Count first. It is cheap — roughly 41 reads on a 6.22 GB index — and it is the only
    // way to know whether ranking is affordable BEFORE committing to it. Guessing from the
    // query text would be guessing about the corpus.
    const total = this.count(query);
    const ranked = total <= rankLimit;

    if (ranked) {
      // bm25() returns a NEGATIVE score where more negative is better, so ascending order
      // puts the best hit first. Negating afterwards gives callers a number that behaves
      // the way a score is expected to.
      const rows = this.db.query<[number, string, string, number]>(
        `SELECT rowid, title, ${snip}, bm25(${this.table})
           FROM ${this.table}
          WHERE ${this.table} MATCH ?
          ORDER BY bm25(${this.table})
          LIMIT ? OFFSET ?`,
        [match, limit, offset]
      );
      return {
        total,
        ranked: true,
        hits: rows.map(([docId, title, snippet, score]) => ({
          docId, title, snippet, score: -score
        }))
      };
    }

    // Too hot to rank: return matches in storage order and say so. Slow honest results beat
    // fast wrong ones, but two seconds of silence beats neither.
    const rows = this.db.query<[number, string, string]>(
      `SELECT rowid, title, ${snip}
         FROM ${this.table}
        WHERE ${this.table} MATCH ?
        LIMIT ? OFFSET ?`,
      [match, limit, offset]
    );
    return {
      total,
      ranked: false,
      hits: rows.map(([docId, title, snippet]) => ({ docId, title, snippet, score: 0 }))
    };
  }

  count(query: string): number {
    const rows = this.db.query<[number]>(
      `SELECT count(*) FROM ${this.table} WHERE ${this.table} MATCH ?`,
      [sanitiseQuery(query)]
    );
    return rows[0]?.[0] ?? 0;
  }
}
