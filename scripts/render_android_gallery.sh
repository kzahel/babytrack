#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
gallery_dir="$repo_root/apps/android/app/build/outputs/screen-gallery"
# Only dedicated generated outputs are cleaned. Fixture sources remain untouched.
python3 - "$gallery_dir" <<'PY'
import shutil
import sys
from pathlib import Path
output = Path(sys.argv[1])
if output.is_symlink():
    raise SystemExit("Gallery output must not be a symlink")
if output.exists():
    shutil.rmtree(output)
output.mkdir(parents=True)
PY
"$repo_root/apps/android/gradlew" :app:recordScreenGallery --no-daemon "$@"
python3 "$repo_root/scripts/build_android_gallery.py" "$gallery_dir"
