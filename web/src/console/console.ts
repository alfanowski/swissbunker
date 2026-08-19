/**
 * The SwissBunker Console.
 *
 * One page, two modes, decided at runtime by whether the daemon answers:
 *
 *   Connected — the daemon is running from the disk. Building, progress and health are
 *               available, and the disk can be filled.
 *   Portable  — no daemon. Read-only: pick the disk with the file input and search it.
 *
 * The same code serves both, and the management controls simply do not appear in Portable.
 * Two codebases would drift, and the drift would only show up on the machine where there is
 * no way to debug it.
 */

interface DiskState {
  disk: string;
  has_bunker: boolean;
  corpora: number;
  documents: number;
  index_bytes: number;
}

interface CorpusEntry {
  id: string;
  name: string;
  language: string;
  documents: number;
  index_bytes: number;
  index_file: string;
  importance_signal: string;
}

interface JobProgress {
  job: string;
  stage: string;
  position: number;
  total: number;
  done: boolean;
  error: string | null;
}

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

/** Format bytes the way a person reads them, not the way a computer stores them. */
function humanBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

/**
 * What an importance signal means for the person reading results.
 *
 * Roughly half of real searches take the unranked path, where results come back in insertion
 * order. Saying which signal produced that order is the difference between "the most
 * important documents containing this word" and "some documents containing this word".
 */
function explainSignal(signal: string): string {
  switch (signal) {
    case 'inbound-links': return 'ordered by how often other articles link to them';
    case 'pageviews': return 'ordered by how often they are read';
    case 'source-order': return 'kept in the order the source file listed them';
    case 'explicit': return 'ordered by values supplied with the corpus';
    case 'none': return 'in no particular order — broad searches will look arbitrary';
    default: return `ordered by ${signal}`;
  }
}

/** Is a daemon answering on this origin? */
async function detectDaemon(): Promise<DiskState | null> {
  try {
    const res = await fetch('/api/state', { signal: AbortSignal.timeout(2000) });
    if (!res.ok) return null;
    return (await res.json()) as DiskState;
  } catch {
    // Being served from file://, or no daemon running. Both mean Portable, and neither is
    // an error worth showing: Portable is a supported way to use the product, not a fallback.
    return null;
  }
}

async function main(): Promise<void> {
  const state = await detectDaemon();
  if (state) {
    renderConnected(state);
  } else {
    renderPortable();
  }
}

// --- Connected -------------------------------------------------------------------------

function renderConnected(state: DiskState): void {
  $('mode').textContent = 'Connected';
  $('mode').className = 'badge connected';
  $('connected').hidden = false;
  $('disk-path').textContent = state.disk;

  refreshSummary(state);
  void refreshCorpora();
  watchProgress();

  $('build-form').addEventListener('submit', onBuild);
}

function refreshSummary(state: DiskState): void {
  $('summary').textContent = state.has_bunker
    ? `${state.corpora} corpora · ${state.documents.toLocaleString()} documents · ${humanBytes(state.index_bytes)} of index`
    : 'This disk holds no bunker yet. Put a .jsonl corpus on it and build below.';
}

async function refreshCorpora(): Promise<void> {
  const m = await (await fetch('/api/manifest')).json();
  const list = $('corpora');
  list.replaceChildren();

  const corpora: CorpusEntry[] = m.corpora ?? [];
  if (corpora.length === 0) return;

  for (const c of corpora) {
    const li = document.createElement('li');
    const h = document.createElement('h3');
    h.textContent = c.name;
    const meta = document.createElement('p');
    meta.className = 'meta';
    meta.textContent =
      `${c.documents.toLocaleString()} documents · ${humanBytes(c.index_bytes)} · ` +
      explainSignal(c.importance_signal);
    li.append(h, meta);
    list.append(li);
  }
  const s: DiskState = await (await fetch('/api/state')).json();
  refreshSummary(s);
}

async function onBuild(e: Event): Promise<void> {
  e.preventDefault();
  const corpus = $<HTMLInputElement>('corpus').value.trim();
  const id = $<HTMLInputElement>('corpus-id').value.trim();
  const name = $<HTMLInputElement>('corpus-name').value.trim();
  if (!corpus || !id) return;

  const button = $<HTMLButtonElement>('build-button');
  button.disabled = true;
  $('build-error').textContent = '';

  const res = await fetch('/api/build', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ corpus, id, name: name || undefined })
  });

  if (!res.ok) {
    // The daemon's message says what to do about it, so it is shown verbatim rather than
    // replaced with something vaguer.
    const body = await res.json().catch(() => ({ error: 'the daemon refused the request' }));
    $('build-error').textContent = body.error ?? 'unknown error';
    button.disabled = false;
    return;
  }
  button.disabled = false;
}

/**
 * Follow the progress stream for as long as the page is open.
 *
 * EventSource reconnects on its own if the daemon restarts, which matters more here than it
 * looks: a build outlives a page reload, and a dashboard that lost track of it would be
 * worse than useless.
 */
function watchProgress(): void {
  const source = new EventSource('/api/progress');
  source.onmessage = (event) => {
    const payload = JSON.parse(event.data) as { jobs?: JobProgress[]; error?: string };
    renderProgress(payload.jobs ?? []);
    // A finished or failed job changes the manifest, so the list is refreshed when the
    // pipeline goes quiet rather than on a timer.
    if ((payload.jobs ?? []).every((j) => j.done || j.error)) {
      void refreshCorpora();
    }
  };
}

function renderProgress(jobs: JobProgress[]): void {
  const box = $('progress');
  if (jobs.length === 0) {
    box.replaceChildren();
    return;
  }

  const frag = document.createDocumentFragment();
  for (const j of jobs) {
    const row = document.createElement('div');
    row.className = 'job';

    const label = document.createElement('span');
    label.className = 'job-label';
    label.textContent = `${j.job} · ${j.stage}`;

    const status = document.createElement('span');
    status.className = j.error ? 'job-status failed' : j.done ? 'job-status done' : 'job-status';
    status.textContent = j.error
      ? j.error
      : j.done
        ? 'done'
        // The total is unknown while streaming, so a count is shown rather than a percentage
        // invented to fill a progress bar.
        : `${j.position.toLocaleString()} so far`;

    row.append(label, status);
    frag.append(row);
  }
  box.replaceChildren(frag);
}

// --- Portable --------------------------------------------------------------------------

function renderPortable(): void {
  $('mode').textContent = 'Portable';
  $('mode').className = 'badge portable';
  $('portable').hidden = false;
  $('summary').textContent =
    'No daemon on this machine — read-only. Select your bunker disk to search it.';
}

void main();
