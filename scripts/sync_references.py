#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pyyaml"]
# ///
"""Clone or update the reference repos listed in references.yaml.

Usage: scripts/sync_references.py [name ...]

Repos are shallow-cloned into references/<name>. Existing clones are updated
with a fast-forward pull. With names, only those repos are synced.
"""

import subprocess
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "references.yaml"
DEST = ROOT / "references"


def git(*args: str, cwd: Path | None = None) -> bool:
    return subprocess.run(["git", *args], cwd=cwd).returncode == 0


def sync(name: str, url: str) -> bool:
    path = DEST / name
    if (path / ".git").exists():
        print(f"== {name}: pull")
        return git("pull", "--ff-only", "--quiet", cwd=path)
    print(f"== {name}: clone {url}")
    return git("clone", "--depth", "1", "--quiet", url, str(path))


def main() -> int:
    repos = yaml.safe_load(MANIFEST.read_text())["repos"]
    wanted = set(sys.argv[1:])
    unknown = wanted - {r["name"] for r in repos}
    if unknown:
        print(f"unknown repo(s): {', '.join(sorted(unknown))}", file=sys.stderr)
        return 2

    DEST.mkdir(exist_ok=True)
    failed = [
        r["name"]
        for r in repos
        if (not wanted or r["name"] in wanted) and not sync(r["name"], r["url"])
    ]
    if failed:
        print(f"failed: {', '.join(failed)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
