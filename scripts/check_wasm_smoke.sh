#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scratch_dir="$(mktemp -d)"
trap 'rm -rf "$scratch_dir"' EXIT

cargo build -p babytrack-core-wasm --features fixture-api --target wasm32-unknown-unknown --locked
wasm-bindgen --target nodejs --out-dir "$scratch_dir" \
  target/wasm32-unknown-unknown/debug/babytrack_core_wasm.wasm
node core-wasm/tests/wasm-smoke.cjs "$scratch_dir/babytrack_core_wasm.js"
