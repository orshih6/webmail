#!/usr/bin/env bash
# Browser walkthroughs against a running server on the dev stack. Each resets alice's and
# bob's prefs, identities, contacts and folders first, so runs are independent.
#   dev/up.sh && source dev/dev.env && cargo run &     then: e2e/run.sh
set -euo pipefail
cd "$(dirname "$0")"
[ -d node_modules ] || npm ci --silent
python3 ../dev/seed.py
# resilience freezes the mail container, so it runs last: nothing after it inherits a
# just-thawed Postfix.
for s in core settings fundamentals session undo search conversation caching a11y resilience; do
  echo "── $s"
  node "$s.mjs"
done
