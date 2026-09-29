#!/usr/bin/env bash
# Opt-in hosted preview check. Clears only the two dedicated disposable AVDs.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
if [[ "${BABYTRACK_DISPOSABLE_PREVIEW:-}" != 1 ]]; then
  echo 'Set BABYTRACK_DISPOSABLE_PREVIEW=1 to use disposable hosted preview data.' >&2
  exit 1
fi
manager="${BABYTRACK_MANAGER_SERIAL:-emulator-5554}"
recipient="${BABYTRACK_RECIPIENT_SERIAL:-emulator-5556}"
if [[ "$manager" == "$recipient" ]]; then
  echo 'Two separate emulator installations are required.' >&2
  exit 1
fi
check_avd() {
  local serial="$1" expected="$2" actual
  if [[ ! "$serial" =~ ^emulator-[0-9]+$ ]]; then
    echo "Refusing non-emulator target $serial" >&2
    exit 1
  fi
  actual="$(adb -s "$serial" emu avd name | tr -d '\r' | head -1)"
  if [[ "$actual" != "$expected" ]]; then
    echo "Expected disposable AVD $expected at $serial; found $actual" >&2
    exit 1
  fi
}
check_avd "$manager" babytrack-sharing-a
check_avd "$recipient" babytrack-sharing-b
scratch="$(mktemp -d "${TMPDIR:-/tmp}/babytrack-preview.XXXXXX")"
pids=()
cleanup() {
  for pid in "${pids[@]:-}"; do
    if [[ -n "$pid" ]]; then kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; fi
  done
  for serial in "$manager" "$recipient"; do
    adb -s "$serial" shell svc wifi enable >/dev/null 2>&1 || true
    adb -s "$serial" shell svc data enable >/dev/null 2>&1 || true
    adb -s "$serial" shell am force-stop org.babytrack.app >/dev/null 2>&1 || true
  done
  echo "Preview check reports: $scratch"
}
trap cleanup EXIT
for serial in "$manager" "$recipient"; do
  adb -s "$serial" shell svc wifi enable
  adb -s "$serial" shell svc data enable
  adb -s "$serial" install -r apps/android/app/build/outputs/apk/debug/app-debug.apk >/dev/null
  adb -s "$serial" install -r apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk >/dev/null
  adb -s "$serial" shell pm clear org.babytrack.app >/dev/null
done
run_step() {
  local serial="$1" step="$2"
  shift 2
  adb -s "$serial" shell am instrument -w \
    -e class "org.babytrack.app.PreviewTwoPhoneTest#$step" \
    -e disposablePreview true "$@" \
    org.babytrack.app.test/androidx.test.runner.AndroidJUnitRunner > "$scratch/$step.txt"
  if ! rg -q '^OK \(1 test\)$' "$scratch/$step.txt"; then
    cat "$scratch/$step.txt" >&2
    adb -s "$serial" logcat -d > "$scratch/$step-logcat.txt" || true
    adb -s "$serial" exec-out screencap -p > "$scratch/$step.png" || true
    return 1
  fi
  echo "$serial $step: OK"
}
run_step "$manager" managerSharesAndCopiesInviteFromUi
link="$(adb -s "$manager" exec-out run-as org.babytrack.app cat files/preview-test-invite.txt)"
if [[ ! "$link" =~ ^babytrack://join#bt-invite=v1\.[A-Za-z0-9_-]+$ ]]; then
  echo 'Invalid copied Android invitation' >&2
  exit 1
fi
run_step "$recipient" recipientStartsJoinFromLinkUi -e inviteLink "'$link'"
run_step "$manager" managerAdvanceJoin
run_step "$recipient" recipientAdvanceJoin
run_step "$manager" managerAdvanceJoin
run_step "$manager" managerWritesNote
run_step "$recipient" recipientReadyAndReadsManagerNote
run_step "$recipient" recipientWritesNote
run_step "$manager" managerReadsRecipientNote
run_step "$manager" managerLogsDiaperFromUi
run_step "$manager" managerConvergesInForeground -e expectedDiapers 1
run_step "$recipient" recipientSyncsAndReadsManagerDiaper
run_step "$recipient" recipientLogsDiaperFromUi
run_step "$recipient" recipientConvergesInForeground -e expectedDiapers 2
run_step "$manager" managerSyncsAndReadsBothDiapers
# Ensure both start the offline pass from the same two-diaper history.
run_step "$recipient" recipientConvergesInForeground -e expectedDiapers 2
for serial in "$manager" "$recipient"; do
  adb -s "$serial" shell am force-stop org.babytrack.app
  adb -s "$serial" shell svc wifi disable
  adb -s "$serial" shell svc data disable
done
run_step "$manager" managerLogsOfflineDiaperFromUi -e expectedDiapers 3
run_step "$recipient" recipientLogsOfflineDiaperFromUi -e expectedDiapers 3
for serial in "$manager" "$recipient"; do
  adb -s "$serial" shell am force-stop org.babytrack.app
done
run_step "$manager" managerChecksOfflineDiapersAfterRestart -e expectedDiapers 3
run_step "$recipient" recipientChecksOfflineDiapersAfterRestart -e expectedDiapers 3
for serial in "$manager" "$recipient"; do
  adb -s "$serial" shell svc wifi enable
  adb -s "$serial" shell svc data enable
done
# No manual sync: each activity's foreground coordinator must upload and read.
run_step "$manager" managerConvergesInForeground -e expectedDiapers 4 &
pids+=("$!")
run_step "$recipient" recipientConvergesInForeground -e expectedDiapers 4 &
pids+=("$!")
for pid in "${pids[@]}"; do wait "$pid"; done
pids=()
echo 'Hosted preview two-emulator UI/offline/foreground checks passed.'
