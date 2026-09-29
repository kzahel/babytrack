#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
bash scripts/build_web.sh
(cd apps/web && npm run preview -- --host 127.0.0.1 --port 4178) > /tmp/babytrack-web-ui-preview.log 2>&1 &
preview_pid=$!
trap 'kill "$preview_pid" 2>/dev/null || true' EXIT
for _ in $(seq 1 50); do
    if curl --fail --silent http://127.0.0.1:4178/ > /dev/null; then break; fi
    sleep 0.1
done
cd tests/browser
node web-ui-smoke.cjs http://127.0.0.1:4178/
