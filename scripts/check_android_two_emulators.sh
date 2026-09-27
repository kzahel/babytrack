#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

manager="${BABYTRACK_MANAGER_SERIAL:-emulator-5554}"
recipient="${BABYTRACK_RECIPIENT_SERIAL:-emulator-5556}"
if [[ "$manager" == "$recipient" ]]; then
  echo "Manager and recipient need separate Android installations" >&2
  exit 1
fi
for serial in "$manager" "$recipient"; do
  if [[ "$(adb -s "$serial" get-state 2>/dev/null)" != device ]]; then
    echo "Android device $serial is unavailable" >&2
    exit 1
  fi
done

scratch="$(mktemp -d)"
relay_pid=""
cleanup() {
  if [[ -n "$relay_pid" ]]; then
    kill "$relay_pid" 2>/dev/null || true
    wait "$relay_pid" 2>/dev/null || true
  fi
  rm -rf "$scratch"
}
trap cleanup EXIT

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
    cat "$scratch/relay.log" >&2
    exit 1
  fi
  sleep 0.1
done
if [[ ! "$public_key" =~ ^[0-9a-f]{64}$ ]]; then
  echo "Relay public key unavailable" >&2
  exit 1
fi

for serial in "$manager" "$recipient"; do
  adb -s "$serial" reverse tcp:8787 tcp:8787 >/dev/null
  adb -s "$serial" install -r apps/android/app/build/outputs/apk/debug/app-debug.apk >/dev/null
  adb -s "$serial" install -r apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk >/dev/null
  adb -s "$serial" shell pm clear org.babytrack.app >/dev/null
done

run_step() {
  local serial="$1"
  local step="$2"
  shift 2
  adb -s "$serial" shell am instrument -w \
    -e class "org.babytrack.app.TwoDeviceRelayTest#$step" "$@" \
    org.babytrack.app.test/androidx.test.runner.AndroidJUnitRunner > "$scratch/$step.txt"
  if ! rg -q '^OK \(1 test\)$' "$scratch/$step.txt"; then
    cat "$scratch/$step.txt" >&2
    exit 1
  fi
  echo "$serial $step: OK"
}

run_step "$manager" managerCreate -e relayPublicKey "$public_key"
fragment="$(adb -s "$manager" exec-out run-as org.babytrack.app cat files/two-device-link.txt)"
if [[ ! "$fragment" =~ ^#bt-invite=v1\.[A-Za-z0-9_-]+$ ]]; then
  echo "Manager invitation fragment invalid" >&2
  exit 1
fi
run_step "$recipient" recipientClaim -e fragment "'$fragment'"
run_step "$manager" managerRespond
run_step "$recipient" recipientProve
run_step "$manager" managerAdmit
run_step "$recipient" recipientReadyAndUpload
run_step "$manager" managerVerifyAndUpload
run_step "$recipient" recipientVerifyAndSaveOffline
run_step "$manager" managerRemove
run_step "$recipient" recipientRemovedAndCopied
if rg --text -q 'Care note marker 67' "$scratch/relay.db" "$scratch/relay.log"; then
  echo "Plaintext note reached relay storage or logs" >&2
  exit 1
fi
if rg --text -q 'Test medicine marker 68' "$scratch/relay.db" "$scratch/relay.log"; then
  echo "Plaintext medication reached relay storage or logs" >&2
  exit 1
fi
if rg --text -q 'Pear marker 69' "$scratch/relay.db" "$scratch/relay.log"; then
  echo 'solids plaintext reached relay' >&2
  exit 1
fi
