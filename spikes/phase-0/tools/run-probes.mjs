// Phase 0 probe runner.
//
// Drives every probe across every available engine, in both the file:// condition and the
// http:// control, and files the resulting records into results/.
//
// Two rules this script must never break, because breaking either makes the whole spike
// worthless:
//
//   1. NO permissive flags. Never pass --allow-file-access-from-files or similar. The
//      spike exists to measure what a browser does to a file:// page BY DEFAULT. A flag
//      that unblocks the very thing being measured turns a finding into a fiction.
//   2. Headed, not headless. The condition under test is "a person opens a file in their
//      browser". Headless Chromium falls back to SwiftShader for WebGPU, which would
//      report software-rendered limits as if they were the machine's real ones.
//
// Usage:
//   node tools/run-probes.mjs --engines chromium,chrome,firefox,webkit --probes p1,p2,p3
//   node tools/run-probes.mjs --only-http     (control condition only)

import { chromium, firefox, webkit } from 'playwright';
import { writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');
const RESULTS = join(ROOT, 'results');
const FIXTURES = process.env.SB_FIXTURES || '/Volumes/SWISSTEST';
const MODEL_DIR = process.env.SB_MODEL_DIR
  || join(process.env.HOME, 'swissbunker-fixtures', 'model');
const PORT = 8391;

// --- argument parsing ------------------------------------------------------------------
const argv = process.argv.slice(2);
function arg(name, fallback) {
  const i = argv.indexOf('--' + name);
  return i >= 0 && argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[i + 1] : fallback;
}
const ONLY_HTTP = argv.includes('--only-http');
const ONLY_FILE = argv.includes('--only-file');
const WANT_ENGINES = arg('engines', 'chromium,chrome,firefox,webkit').split(',');
const WANT_PROBES = arg('probes', 'p1,p2,p3,p4,p5,p6').split(',');
const TIMEOUT_MS = Number(arg('timeout', '900')) * 1000;

const ENGINES = {
  // Playwright's bundled Chromium and the user's real Chrome are different builds; both
  // are recorded because they can diverge on exactly the policies under test.
  chromium: { launcher: chromium, opts: {} },
  chrome:   { launcher: chromium, opts: { channel: 'chrome' } },
  // Playwright's Firefox build ships playwright.cfg with
  //   pref("security.fileuri.strict_origin_policy", false)
  // which switches OFF the very policy this spike measures. Left alone it reports a
  // file:// page as same-origin with its whole directory, so ES modules and fetch appear
  // to work when on a real Firefox they would not. Restoring it to true is mandatory:
  // without this line every Firefox file:// record is fiction.
  firefox:  { launcher: firefox,
              opts: { firefoxUserPrefs: { 'security.fileuri.strict_origin_policy': true } } },
  // Playwright's WebKit is NOT Safari. It shares the engine but not the browser's own
  // policy layer, so a WebKit result is evidence about Safari, never proof.
  webkit:   { launcher: webkit,   opts: {} }
};

const PROBES = {
  p1: { file: 'p1-module-loading.html',    input: null },
  p2: { file: 'p2-directory-picker.html',  input: FIXTURES },
  p3: { file: 'p3-range-read.html',        input: FIXTURES },
  p4: { file: 'p4-sqlite-lazy.html',       input: FIXTURES },
  p5: { file: 'p5-webgpu-worker.html',     input: null },
  p6: { file: 'p6-model-weights.html',     input: MODEL_DIR }
};

// --- control-condition server ------------------------------------------------------------
function startServer() {
  const proc = spawn('python3', ['-m', 'http.server', String(PORT)], {
    cwd: ROOT, stdio: 'ignore', detached: true
  });
  return proc;
}

// --- one probe run -----------------------------------------------------------------------
async function runProbe(engineName, probeId, protocol) {
  const engine = ENGINES[engineName];
  const probe = PROBES[probeId];
  const label = `${probeId} ${engineName} ${protocol}`;

  let browser;
  try {
    browser = await engine.launcher.launch({ headless: false, ...engine.opts });
  } catch (err) {
    console.log(`  SKIP ${label} — engine unavailable: ${err.message.split('\n')[0]}`);
    return null;
  }

  const consoleErrors = [];
  let record = null;

  try {
    const page = await browser.newPage();
    page.on('console', m => { if (m.type() === 'error') consoleErrors.push(m.text()); });
    page.on('pageerror', e => consoleErrors.push('pageerror: ' + e.message));

    const url = protocol === 'file'
      ? 'file://' + join(ROOT, probe.file)
      : `http://localhost:${PORT}/${probe.file}`;

    await page.goto(url, { timeout: 60000 });

    if (probe.input) {
      if (!existsSync(probe.input)) {
        console.log(`  SKIP ${label} — fixture directory missing: ${probe.input}`);
        await browser.close();
        return null;
      }
      // Playwright sets the FileList directly through the protocol, so no native dialog
      // is involved. For a [webkitdirectory] input it accepts a single directory path.
      await page.locator('#dir-input').setInputFiles(probe.input, { timeout: 120000 });
    }

    // Probe.finish() appends the download anchor; its appearance is the completion signal.
    let complete = true;
    try {
      await page.waitForSelector('a[download]', { timeout: TIMEOUT_MS });
    } catch {
      complete = false;
      console.log(`  WARN ${label} — no completion signal within ${TIMEOUT_MS / 1000}s, filing partial`);
    }

    const text = await page.evaluate(() => {
      const el = document.getElementById('probe-output');
      return el ? el.textContent : null;
    });

    if (!text) {
      console.log(`  FAIL ${label} — probe produced no output`);
      await browser.close();
      return null;
    }

    record = JSON.parse(text);
    record._provenance = {
      engine: engineName,
      engineNote: engineName === 'webkit'
        ? 'Playwright WebKit — shares Safari\'s engine, not its policy layer'
        : engineName === 'chromium'
          ? 'Playwright bundled Chromium, not the user\'s Chrome build'
          : undefined,
      protocol,
      os: 'macOS',
      machine: 'Apple M4, 16 GB',
      headless: false,
      permissiveFlags: engineName === 'firefox'
        ? 'none; security.fileuri.strict_origin_policy restored to true'
        : 'none',
      complete
    };
    if (consoleErrors.length) { record._consoleErrors = consoleErrors.slice(0, 20); }

    const checks = record.checks || {};
    const pass = Object.values(checks).filter(c => c.ok).length;
    console.log(`  ${complete ? 'DONE' : 'PART'} ${label} — ${pass}/${Object.keys(checks).length} checks pass`);
  } catch (err) {
    console.log(`  FAIL ${label} — ${err.message.split('\n')[0]}`);
  } finally {
    await browser.close().catch(() => {});
  }

  return record;
}

// --- main --------------------------------------------------------------------------------
mkdirSync(RESULTS, { recursive: true });
const server = startServer();
await new Promise(r => setTimeout(r, 1500));

const protocols = ONLY_HTTP ? ['http'] : ONLY_FILE ? ['file'] : ['file', 'http'];
console.log(`Fixtures: ${FIXTURES}`);
console.log(`Models:   ${MODEL_DIR}`);
console.log(`Running ${WANT_PROBES.join(',')} on ${WANT_ENGINES.join(',')} over ${protocols.join(',')}\n`);

for (const engineName of WANT_ENGINES) {
  if (!ENGINES[engineName]) { console.log(`unknown engine: ${engineName}`); continue; }
  console.log(`== ${engineName} ==`);
  for (const probeId of WANT_PROBES) {
    if (!PROBES[probeId]) { console.log(`unknown probe: ${probeId}`); continue; }
    for (const protocol of protocols) {
      const rec = await runProbe(engineName, probeId, protocol);
      if (rec) {
        const name = `${probeId}-${protocol}-${engineName}-macos.json`;
        writeFileSync(join(RESULTS, name), JSON.stringify(rec, null, 2));
      }
    }
  }
  console.log('');
}

try { process.kill(-server.pid); } catch { /* already gone */ }
console.log('Done. Records in results/');
process.exit(0);
