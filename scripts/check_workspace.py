#!/usr/bin/env python3
"""Check scaffold package placement and the Rust dependency boundary."""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
BOUNDARIES = {
    "babytrack-core": "core",
    "babytrack-core-ffi": "core-ffi",
    "babytrack-core-wasm": "core-wasm",
    "babytrack-server": "server",
    "babytrack-cli": "cli",
}


def main() -> None:
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1"],
            cwd=ROOT,
            text=True,
        )
    )
    packages = {package["id"]: package for package in metadata["packages"]}
    members = {
        packages[package_id]["name"]: packages[package_id]
        for package_id in metadata["workspace_members"]
    }
    missing = BOUNDARIES.keys() - members.keys()
    if missing:
        raise SystemExit(f"Missing workspace boundaries: {sorted(missing)}")

    for name, directory in BOUNDARIES.items():
        actual = Path(members[name]["manifest_path"]).resolve().parent
        if actual != ROOT / directory:
            raise SystemExit(f"{name} must live in {directory}/, found {actual}")

    core_id = members["babytrack-core"]["id"]
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}

    def reaches_core(package_id: str, seen: set[str] | None = None) -> bool:
        seen = set() if seen is None else seen
        if package_id in seen:
            return False
        seen.add(package_id)
        for dependency in nodes[package_id]["deps"]:
            dep_id = dependency["pkg"]
            if dep_id == core_id or reaches_core(dep_id, seen):
                return True
        return False

    for name in ("babytrack-core-ffi", "babytrack-core-wasm", "babytrack-cli"):
        if not any(
            dep["pkg"] == core_id for dep in nodes[members[name]["id"]]["deps"]
        ):
            raise SystemExit(f"{name} must depend directly on babytrack-core")

    if reaches_core(members["babytrack-server"]["id"]):
        raise SystemExit("babytrack-server must not depend on babytrack-core")

    print("Workspace boundaries and core dependency direction: OK")


if __name__ == "__main__":
    main()
