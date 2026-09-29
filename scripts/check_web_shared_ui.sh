#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
bash scripts/build_web.sh
cargo build -p babytrack-server --locked
cargo build -p babytrack-cli --example browser_holder --locked
node tests/browser/web-sharing-ui.cjs target/debug/babytrack-server target/debug/examples/browser_holder
