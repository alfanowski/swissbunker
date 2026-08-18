// Print what each library actually offers for building a custom VFS.
//
// This runs BEFORE any implementation is written. Phase 0 was burned by coding against an
// invented FS.createLazyFile signature that did not exist in the shipped build, and the cost
// was a probe that could never have passed. Nothing here is kept.
import { createRequire } from 'node:module';
import { existsSync, readdirSync, statSync } from 'node:fs';
const require = createRequire(import.meta.url);

const kb = (p) => (existsSync(p) ? Math.round(statSync(p).size / 1024) + ' KB' : '—');

console.log('=== @sqlite.org/sqlite-wasm ===');
try {
  const mod = await import('@sqlite.org/sqlite-wasm');
  const sqlite3 = await mod.default();
  console.log('  version:', sqlite3.version.libVersion);

  const vfsSyms = Object.keys(sqlite3.capi).filter(k => /vfs/i.test(k)).sort();
  console.log(`  capi vfs symbols (${vfsSyms.length}):`);
  vfsSyms.forEach(k => console.log('     ', k));

  // The struct binding layer decides whether a JS VFS is practical here: without it, a
  // custom VFS means hand-writing sqlite3_vfs and sqlite3_io_methods into wasm memory.
  console.log('  StructBinder:', typeof sqlite3.StructBinder);
  if (sqlite3.StructBinder) {
    const structs = Object.keys(sqlite3.StructBinder).filter(k => /^sqlite3/.test(k));
    console.log('    struct types:', structs.join(', ') || '(none enumerable)');
  }
  console.log('  sqlite3.vfs namespace:', typeof sqlite3.vfs,
              sqlite3.vfs ? Object.keys(sqlite3.vfs).join(', ') : '');
  console.log('  sqlite3.oo1 helpers:', Object.keys(sqlite3.oo1 || {}).join(', '));

  // Does it expose the io-methods struct we would have to fill in?
  const io = Object.keys(sqlite3.capi).filter(k => /io_methods|_file$/i.test(k));
  console.log('  io_methods symbols:', io.join(', ') || 'none');
} catch (e) {
  console.log('  inspection failed:', e.message.split('\n')[0]);
}

console.log('\n  packaging');
const offDir = 'node_modules/@sqlite.org/sqlite-wasm/sqlite-wasm/jswasm';
if (existsSync(offDir)) {
  readdirSync(offDir).forEach(f => console.log(`     ${f.padEnd(38)} ${kb(offDir + '/' + f)}`));
}

console.log('\n=== wa-sqlite ===');
try {
  const pkg = require('wa-sqlite/package.json');
  console.log('  version:', pkg.version, '| type:', pkg.type || 'commonjs');
  console.log('  exports:', Object.keys(pkg.exports || {}).join(', ') || '(none declared)');

  // wa-sqlite's whole premise: a base class you subclass with xRead/xOpen/xFileSize.
  // If that class is there, writing a VFS is subclassing rather than struct surgery.
  for (const path of ['wa-sqlite/src/VFS.js', 'wa-sqlite/src/FacadeVFS.js']) {
    if (!existsSync('node_modules/' + path)) { console.log(`  ${path}: absent`); continue; }
    const mod = await import(path);
    const cls = mod.Base || mod.FacadeVFS || mod.default;
    if (!cls) { console.log(`  ${path}: no class exported, keys = ${Object.keys(mod).join(', ')}`); continue; }
    const methods = Object.getOwnPropertyNames(cls.prototype)
      .filter(m => m !== 'constructor').sort();
    console.log(`  ${path} -> ${cls.name}, ${methods.length} methods`);
    console.log('    ', methods.join(', '));
  }
} catch (e) {
  console.log('  inspection failed:', e.message.split('\n')[0]);
}

console.log('\n  packaging');
const waDir = 'node_modules/wa-sqlite/dist';
if (existsSync(waDir)) {
  readdirSync(waDir).forEach(f => console.log(`     ${f.padEnd(38)} ${kb(waDir + '/' + f)}`));
}
