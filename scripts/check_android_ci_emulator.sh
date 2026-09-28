#!/usr/bin/env bash
set -euo pipefail

bash scripts/check_android_relay_emulator.sh
if [[ "${GITHUB_EVENT_NAME:-}" == schedule || "${GITHUB_EVENT_NAME:-}" == workflow_dispatch ]]; then
  python3 scripts/check_android_ui_smoke.py
else
  python3 scripts/check_android_ui_smoke.py --quick
fi
python3 scripts/check_android_recovery_ui.py
