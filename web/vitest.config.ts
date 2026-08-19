import { defineConfig } from 'vitest/config';

// Unit tests run in a real browser, not Node. Everything this code touches — Blob,
// URL.createObjectURL, synchronous XMLHttpRequest, File — exists only there, so a Node run
// would pass against stubs and prove nothing.
//
// Note this is the http:// CONTROL condition, not the target. The assertions that only mean
// something under a null origin live in test/conformance instead.
export default defineConfig({
  test: {
    globals: true,
    include: ['test/unit/**/*.test.ts'],
    browser: {
      enabled: true,
      provider: 'playwright',
      name: 'chromium',
      headless: true
    }
  }
});
