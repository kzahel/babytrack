#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scratch_dir="$(mktemp -d)"
trap 'rm -rf "$scratch_dir"' EXIT

npm ci --prefix tests/browser --ignore-scripts
cargo build -p babytrack-core-wasm --features fixture-api --target wasm32-unknown-unknown --locked
cargo build -p babytrack-server --locked
cargo build -p babytrack-cli --example browser_exchange --locked
cargo build -p babytrack-cli --example seed_browser_recipient --locked
cargo build -p babytrack-cli --example browser_holder --locked
cargo build -p babytrack-cli --example fixture_relay --locked
wasm-bindgen --target web --out-dir "$scratch_dir" \
  target/wasm32-unknown-unknown/debug/babytrack_core_wasm.wasm
node tests/browser/browser-smoke.cjs "$scratch_dir" target/debug/babytrack-server target/debug/examples/browser_exchange target/debug/examples/seed_browser_recipient target/debug/examples/browser_holder target/debug/examples/fixture_relay "$@"
