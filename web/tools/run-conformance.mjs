// Drive the conformance pages from file://, which is the condition that matters and the one
// Vitest cannot reach — it serves over http.
//
// Rules inherited from the Phase 0 runner, both non-negotiable:
//   1. NO permissive flags. Never --allow-file-access-from-files. A flag that unblocks the
//      thing being measured turns a finding into fiction.
//   2. Headed, not headless. The condition under test is a person opening a file.
import { chromium } from 'playwright';
import { writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');
const FIXTURES = process.env.SB_FIXTURES || '/Volumes/SWISSTEST';
const OUT = join(ROOT, 'test', 'conformance', 'results');
const TIMEOUT = Number(process.env.SB_TIMEOUT || 600) * 1000;

if (!existsSync(FIXTURES)) {
  console.error(`fixtures not mounted at ${FIXTURES}`);
  console.error('mount with: hdiutil attach ~/swissbunker-fixtures/exfat-test.sparseimage');
  process.exit(1);
}
mkdirSync(OUT, { recursive: true });

const page_ = join(ROOT, 'test', 'conformance', 'reader.html');
const browser = await chromium.launch({ headless: false });
const page = await browser.newPage();

const errors = [];
page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
page.on('pageerror', e => errors.push('pageerror: ' + e.message));

console.log(`opening file://${page_}`);
await page.goto('file://' + page_);
await page.locator('#dir-input').setInputFiles(FIXTURES, { timeout: 120000 });

let complete = true;
try {
  await page.waitForSelector('a[download]', { timeout: TIMEOUT });
} catch {
  complete = false;
  console.log(`WARN: no completion signal within ${TIMEOUT / 1000}s, filing partial`);
}

const text = await page.evaluate(() => document.getElementById('probe-output').textContent);
await browser.close();

const record = JSON.parse(text);
record._provenance = {
  engine: 'chromium (playwright)', protocol: 'file', os: 'macOS',
  machine: 'Apple M4, 16 GB', headless: false, permissiveFlags: 'none',
  fixtures: FIXTURES, complete
};
if (errors.length) record._consoleErrors = errors.slice(0, 20);

writeFileSync(join(OUT, 'c1-file-chromium-macos.json'), JSON.stringify(record, null, 2));

const checks = record.checks || {};
for (const [name, c] of Object.entries(checks)) {
  const detail = typeof c.detail === 'string' ? c.detail : '';
  console.log(`  ${c.ok ? 'PASS' : 'FAIL'}  ${name}${c.ok ? '' : ' -> ' + detail.slice(0, 60)}`);
}
for (const [name, m] of Object.entries(record.measurements || {})) {
  console.log(`  ${name}: p50 ${m.p50} ms / p95 ${m.p95} ms`);
}
console.log('\ninfo:', JSON.stringify(record.info, null, 1));

const failed = Object.values(checks).filter(c => !c.ok).length;
process.exit(failed === 0 && complete ? 0 : 1);
