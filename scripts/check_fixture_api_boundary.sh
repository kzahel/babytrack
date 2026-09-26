#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scratch_dir="$(mktemp -d)"
trap 'rm -rf "$scratch_dir"' EXIT
mkdir -p "$scratch_dir/wasm" "$scratch_dir/native"

cargo build -p babytrack-core-wasm --target wasm32-unknown-unknown --no-default-features --locked
wasm-bindgen --target nodejs --out-dir "$scratch_dir/wasm" \
  target/wasm32-unknown-unknown/debug/babytrack_core_wasm.wasm
node - "$scratch_dir/wasm/babytrack_core_wasm.js" <<'JS'
const binding = require(process.argv[2]);
for (const name of ['seal_one', 'ed25519_public_key', 'WasmFamily']) {
  if (name in binding) throw new Error(`production wasm exports fixture primitive: ${name}`);
}
if (!('WasmLocalFamily' in binding)) throw new Error('local projection binding is absent');
JS

cargo build -p babytrack-core-ffi --no-default-features --locked
case "$(uname -s)" in
  Darwin) library_path=target/debug/libbabytrack_core_ffi.dylib ;;
  Linux) library_path=target/debug/libbabytrack_core_ffi.so ;;
  *) echo "unsupported native boundary platform" >&2; exit 1 ;;
esac
cargo run -p babytrack-core-ffi --features bindgen --bin uniffi-bindgen -- \
  generate "$library_path" --language kotlin --out-dir "$scratch_dir/native" --no-format
if rg -q 'NativeFamily|sealOne|ed25519PublicKey' "$scratch_dir/native"; then
  echo "production native binding exports fixture primitives" >&2
  exit 1
fi
echo "production bindings exclude fixture-only crypto and replay APIs: OK"
