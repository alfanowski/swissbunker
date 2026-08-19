// Minimal search UI.
//
// Phase 5 replaces this entirely with the Console. Its only job is to prove the stack works
// end to end under a real person's hands.
import { FileSource } from '../io/source';
import { openDatabase, type ReaderDatabase } from '../sqlite/open';
import { FtsIndex } from '../search/fts';
import { sqliteWasmBinary } from '../generated/wasm-binary';

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

let index: FtsIndex | null = null;
let db: ReaderDatabase | null = null;
let pending = 0;

$('disk').addEventListener('change', async (e) => {
  const files = Array.from((e.target as HTMLInputElement).files ?? []);
  // Largest .sqlite wins: a bunker disk holds several indexes and the main one is the big
  // one. Guessing by name would break the first time a corpus is renamed.
  const dbFile = files
    .filter(f => f.name.endsWith('.sqlite'))
    .sort((a, b) => b.size - a.size)[0];

  if (!dbFile) {
    // An empty state must say what to do, not just report failure — the person reading it
    // may have no internet to look anything up with.
    $('status').textContent =
      'No index on that disk. Expected a .sqlite file at the top level.';
    return;
  }

  $('status').textContent = `Opening ${dbFile.name} (${(dbFile.size / 1e9).toFixed(1)} GB)…`;
  const t0 = performance.now();
  try {
    db = await openDatabase(new FileSource(dbFile), { wasmBinary: sqliteWasmBinary() });
    index = new FtsIndex(db);
  } catch (err) {
    $('status').textContent = `Could not open ${dbFile.name}: ${(err as Error).message}`;
    return;
  }
  $('status').textContent =
    `Ready — ${dbFile.name}, opened in ${Math.round(performance.now() - t0)} ms`;
  $('search-area').hidden = false;
  $<HTMLInputElement>('q').focus();
});

$('q').addEventListener('input', (e) => {
  const query = (e.target as HTMLInputElement).value.trim();
  // Debounce: every keystroke otherwise costs a full index lookup, and on a USB disk that is
  // real I/O rather than a cheap in-memory scan.
  window.clearTimeout(pending);
  pending = window.setTimeout(() => runSearch(query), 150);
});

function runSearch(query: string): void {
  const results = $('results');
  if (!index || query.length < 2) {
    results.replaceChildren();
    $('meta').textContent = '';
    return;
  }

  const t0 = performance.now();
  const result = index.searchDetailed(query, { limit: 20 });
  const hits = result.hits;
  const ms = Math.round(performance.now() - t0);

  // An unranked result set means "twenty documents containing this word", not "the twenty
  // best". Presenting the two identically would mislead someone who has no way to check.
  $('meta').textContent = !hits.length
    ? `Nothing in the bunker matches “${query}” — ${ms} ms`
    : result.ranked
      ? `${result.total} results in ${ms} ms`
      : `${result.total.toLocaleString()} results — too common to rank, showing the first ${hits.length} (${ms} ms)`;

  const frag = document.createDocumentFragment();
  for (const hit of hits) {
    const li = document.createElement('li');
    const h = document.createElement('h2');
    h.textContent = hit.title;                 // titles are user data: never innerHTML
    const p = document.createElement('p');
    // The snippet's only markup is the <mark> pair we passed to snippet() ourselves, and
    // FTS5 escapes the document text around it. Still parsed as a fragment rather than
    // assigned to innerHTML, so nothing else in it can execute.
    p.append(...parseSnippet(hit.snippet));
    li.append(h, p);
    frag.append(li);
  }
  results.replaceChildren(frag);
}

/** Turn a snippet into text nodes plus <mark> elements, ignoring any other markup. */
function parseSnippet(snippet: string): Node[] {
  const out: Node[] = [];
  const re = /<mark>(.*?)<\/mark>/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(snippet)) !== null) {
    if (m.index > last) { out.push(document.createTextNode(snippet.slice(last, m.index))); }
    const mark = document.createElement('mark');
    mark.textContent = m[1] ?? '';
    out.push(mark);
    last = re.lastIndex;
  }
  if (last < snippet.length) { out.push(document.createTextNode(snippet.slice(last))); }
  return out;
}
