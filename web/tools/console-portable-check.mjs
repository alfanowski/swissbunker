// The Console must degrade to Portable when no daemon answers — which is the normal state on
// a machine that is not the owner's, not an error condition.
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const page_ = resolve(here, '..', 'dist', 'index.html');

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage();
const errors = [];
page.on('pageerror', e => errors.push('pageerror: ' + e.message));

// Opened from file://, exactly as someone would on a borrowed computer.
await page.goto('file://' + page_, { waitUntil: 'domcontentloaded' });
await page.waitForSelector('#portable:not([hidden])', { timeout: 10000 });

const out = {
  mode: (await page.textContent('#mode'))?.trim(),
  summary: (await page.textContent('#summary'))?.trim(),
  portableVisible: await page.isVisible('#portable'),
  connectedHidden: !(await page.isVisible('#connected')),
  pageErrors: errors
};
console.log(JSON.stringify(out, null, 1));
await browser.close();
process.exit(out.mode === 'Portable' && out.connectedHidden && errors.length === 0 ? 0 : 1);
