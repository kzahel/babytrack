#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="${1:?expected generated output directory}"
mkdir -p "$out_dir/kotlin" "$out_dir/jniLibs"
cd "$repo_root"

cargo build -p babytrack-core-ffi --features bindgen --locked
case "$(uname -s)" in
  Darwin) library_path=target/debug/libbabytrack_core_ffi.dylib ;;
  Linux) library_path=target/debug/libbabytrack_core_ffi.so ;;
  *) echo "unsupported host for UniFFI generation" >&2; exit 1 ;;
esac
cargo run -p babytrack-core-ffi --features bindgen --bin uniffi-bindgen --locked -- \
  generate "$library_path" --language kotlin --out-dir "$out_dir/kotlin" --no-format
cargo ndk -t arm64-v8a -t x86_64 -o "$out_dir/jniLibs" \
  build --release -p babytrack-core-ffi --locked
