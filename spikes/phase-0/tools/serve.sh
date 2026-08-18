#!/usr/bin/env bash
# The http:// control condition. Every probe must be run twice — here and from file:// —
# because a probe that fails in both places has a bug, while a probe that fails only under
# file:// has found what we are looking for.
set -euo pipefail
cd "$(dirname "$0")/.."
echo "Control baseline: http://localhost:8000"
echo "Run the same probe from file:// and compare the two JSON records."
python3 -m http.server 8000
