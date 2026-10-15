#!/usr/bin/env python3
"""Keep third-party actions immutable throughout Android privileged workflows."""

from pathlib import Path
import re


root = Path(__file__).resolve().parents[1]
for workflow in ("android-internal.yml", "android-publish.yml"):
    path = root / ".github/workflows" / workflow
    for number, line in enumerate(path.read_text().splitlines(), 1):
        match = re.match(r"\s*(?:-\s*)?uses:\s+([^\s#]+)", line)
        if match and not re.fullmatch(r"[\w.-]+/[\w./-]+@[a-f0-9]{40}", match[1]):
            raise SystemExit(f"{path.relative_to(root)}:{number}: pin action to a full commit SHA")
print("Android release actions are pinned to immutable commits")
