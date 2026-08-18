#!/usr/bin/env bash
# Pre-bundles third-party libraries into classic IIFE scripts with inlined wasm, for the
# case where file:// blocks module loading and fetch.
#
# The output is committed on purpose: a probe must run by opening a file, with no build
# step in between, because "needs a build step" is itself one of the failure modes being
# measured.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v npx >/dev/null || { echo "npx required"; exit 1; }

if [ ! -f node_modules/sql.js/dist/sql-wasm.js ]; then
  echo "sql.js not installed. Run first:"
  echo "  cd $(pwd) && npm init -y && npm install sql.js@1.11.0"
  exit 1
fi

mkdir -p vendor
npx --yes esbuild@0.23.0 \
  --bundle \
  --format=iife \
  --global-name=SqlJs \
  --loader:.wasm=base64 \
  --outfile=vendor/sql-wasm.iife.js \
  node_modules/sql.js/dist/sql-wasm.js

echo "Built vendor/sql-wasm.iife.js ($(du -h vendor/sql-wasm.iife.js | cut -f1))"
