#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

relay_pid=""
scratch="$(mktemp -d)"
cleanup() {
  if [[ -n "$relay_pid" ]]; then
    kill "$relay_pid" 2>/dev/null || true
    wait "$relay_pid" 2>/dev/null || true
  fi
  rm -rf "$scratch"
}
trap cleanup EXIT

if [[ -n "${BABYTRACK_RELAY_PUBLIC_KEY:-}" ]]; then
  public_key="$BABYTRACK_RELAY_PUBLIC_KEY"
else
  python3 - "$scratch/seed" <<'PY'
import os
import pathlib
import sys
pathlib.Path(sys.argv[1]).write_bytes(os.urandom(32))
PY
  cargo build -p babytrack-server --locked
  target/debug/babytrack-server "$scratch/relay.db" "$scratch/seed" 127.0.0.1:8787 > "$scratch/relay.log" 2>&1 &
  relay_pid=$!
  public_key=""
  for _ in {1..100}; do
    public_key="$(sed -n 's/^Relay public key: \([0-9a-f]\{64\}\)$/\1/p' "$scratch/relay.log" | head -1)"
    if [[ -n "$public_key" ]]; then break; fi
    if ! kill -0 "$relay_pid" 2>/dev/null; then
      cat "$scratch/relay.log"
      exit 1
    fi
    sleep 0.1
  done
fi

if [[ ! "$public_key" =~ ^[0-9a-f]{64}$ ]]; then
  echo "Relay public key unavailable" >&2
  exit 1
fi

adb reverse tcp:8787 tcp:8787
adb install -r apps/android/app/build/outputs/apk/debug/app-debug.apk
adb install -r apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb shell pm clear org.babytrack.app >/dev/null
adb shell am instrument -w -e class org.babytrack.app.SharingRelayTest -e relayPublicKey "$public_key" \
  org.babytrack.app.test/androidx.test.runner.AndroidJUnitRunner | tee "$scratch/instrument.txt"
rg -q '^OK \([1-9][0-9]* tests\)$' "$scratch/instrument.txt"
