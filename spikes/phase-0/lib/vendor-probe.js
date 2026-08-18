// Dual-format probe target: the classic path sets a global, the module path is imported
// dynamically. Loading this file two different ways is what P1 measures.
window.__VENDOR_CLASSIC_LOADED__ = true;
export const marker = 'esm-loaded';
