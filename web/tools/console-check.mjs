// Verify the Console renders and reaches the daemon, in a real browser over http://.
// The Connected mode is the one that needs a browser to prove: Portable was already covered
// by the phase 1 conformance suite.
import { chromium } from 'playwright';

const base = process.env.SB_BASE || 'http://127.0.0.1:7892';
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage();
const errors = [];
page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
page.on('pageerror', e => errors.push('pageerror: ' + e.message));

// NOT networkidle: the Console opens an EventSource for progress and keeps it open for as
// long as the page lives, so the network is never idle by design. Waiting for the element
// that only appears once the daemon answered is both faster and a stronger assertion.
await page.goto(base, { waitUntil: 'domcontentloaded' });
await page.waitForSelector('#connected:not([hidden])', { timeout: 10000 });

const mode = await page.textContent('#mode');
const summary = await page.textContent('#summary');
const connectedVisible = await page.isVisible('#connected');
const corpora = await page.$$eval('#corpora li h3', els => els.map(e => e.textContent));
const meta = await page.$$eval('#corpora li .meta', els => els.map(e => e.textContent));
const buildFormVisible = await page.isVisible('#build-form');

console.log(JSON.stringify({
  mode: mode?.trim(),
  summary: summary?.trim(),
  connectedVisible,
  buildFormVisible,
  corpora,
  meta,
  consoleErrors: errors
}, null, 1));

await browser.close();
process.exit(errors.length === 0 && mode?.trim() === 'Connected' ? 0 : 1);
