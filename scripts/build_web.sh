#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
cargo build -p babytrack-core-wasm --target wasm32-unknown-unknown --release --locked
mkdir -p apps/web/src/generated
wasm-bindgen --target web --out-dir apps/web/src/generated \
  target/wasm32-unknown-unknown/release/babytrack_core_wasm.wasm
cd apps/web
npm ci --silent
npm test
npm run build
