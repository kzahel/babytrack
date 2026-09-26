#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scratch_dir="$(mktemp -d)"
trap 'rm -rf "$scratch_dir"' EXIT

cargo build -p babytrack-core-ffi --features fixture-api --locked
case "$(uname -s)" in
  Darwin) library_path=target/debug/libbabytrack_core_ffi.dylib ;;
  Linux) library_path=target/debug/libbabytrack_core_ffi.so ;;
  *) echo "unsupported native smoke platform" >&2; exit 1 ;;
esac

bindgen=(cargo run -p babytrack-core-ffi --features bindgen,fixture-api --bin uniffi-bindgen -- generate "$library_path")
"${bindgen[@]}" --language kotlin --out-dir core-ffi/tests/kotlin/build/generated --no-format
core-ffi/tests/kotlin/gradlew -p core-ffi/tests/kotlin run --no-daemon

if [[ "$(uname -s)" == Darwin ]]; then
  "${bindgen[@]}" --language swift --out-dir "$scratch_dir" --no-format
  swiftc \
    -Xcc "-fmodule-map-file=$scratch_dir/babytrack_core_ffiFFI.modulemap" \
    -I "$scratch_dir" -L target/debug -lbabytrack_core_ffi \
    "$scratch_dir/babytrack_core_ffi.swift" core-ffi/tests/native-smoke.swift \
    -o "$scratch_dir/native-smoke"
  DYLD_LIBRARY_PATH=target/debug "$scratch_dir/native-smoke" "$repo_root"
fi
