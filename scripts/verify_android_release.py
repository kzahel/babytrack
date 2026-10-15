#!/usr/bin/env python3
"""Require a tagged Android build and exact-source CI before Play access."""

import json
import os
import re
import subprocess

from release_android import read_version, tag_version


def verify_build(run, repository, tag, source):
    tag_version(tag)
    expected = {
        "event": "push", "head_branch": tag, "head_sha": source,
        "path": ".github/workflows/android-internal.yml",
        "status": "completed", "conclusion": "success",
    }
    if run.get("head_repository", {}).get("full_name") != repository:
        raise ValueError("Build must come from this repository")
    for key, value in expected.items():
        if run.get(key) != value:
            raise ValueError(f"Tagged build differs: {key}")


def has_source_checks(runs, repository, source):
    return any(
        run.get("head_repository", {}).get("full_name") == repository
        and run.get("head_sha") == source and run.get("head_branch") == "main"
        and run.get("event") in ("push", "schedule", "workflow_dispatch")
        and run.get("path") == ".github/workflows/scaffold.yml"
        and run.get("status") == "completed" and run.get("conclusion") == "success"
        for run in runs
    )


def main():
    tag = os.environ["RELEASE_TAG"]
    version = tag_version(tag)
    run_id = os.environ["BUILD_RUN"]
    code = os.environ["VERSION_CODE"]
    if not re.fullmatch(r"[1-9][0-9]*", run_id) or not re.fullmatch(r"[1-9][0-9]*", code):
        raise ValueError("Invalid build run ID or version code")
    repository = os.environ["GITHUB_REPOSITORY"]

    def command(*args):
        return subprocess.check_output(args, text=True).strip()

    def api(path):
        return json.loads(command("gh", "api", f"repos/{repository}/{path}"))

    # Resolve annotated or lightweight tags to a commit; never trust a supplied SHA.
    command("git", "fetch", "--no-tags", "origin", f"refs/tags/{tag}:refs/tags/{tag}")
    source = command("git", "rev-parse", f"refs/tags/{tag}^{{commit}}")
    command("git", "merge-base", "--is-ancestor", source, "origin/main")
    tracked = command("git", "show", f"{source}:apps/android/version.properties")
    if read_version(tracked) != (version, int(code)):
        raise ValueError("Tag/version code differ from the tagged source")
    verify_build(api(f"actions/runs/{run_id}"), repository, tag, source)
    checks = api(f"actions/workflows/scaffold.yml/runs?head_sha={source}&status=success&per_page=100")
    if not has_source_checks(checks["workflow_runs"], repository, source):
        raise ValueError("Full scaffold checks must pass on main for the exact tagged source")
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write(f"source={source}\n")


if __name__ == "__main__":
    main()
