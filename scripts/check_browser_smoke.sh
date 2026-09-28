#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scratch_dir="$(mktemp -d)"
trap 'rm -rf "$scratch_dir"' EXIT

npm ci --prefix tests/browser --ignore-scripts
cargo build -p babytrack-core-wasm --features fixture-api --target wasm32-unknown-unknown --locked
cargo build -p babytrack-server --locked
wasm-bindgen --target web --out-dir "$scratch_dir" \
  target/wasm32-unknown-unknown/debug/babytrack_core_wasm.wasm
node tests/browser/browser-smoke.cjs "$scratch_dir" target/debug/babytrack-server
