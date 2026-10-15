#!/usr/bin/env python3
"""Release Android from main with a version bump and an android-v tag."""

import argparse
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[1]
VERSION_FILE = Path("apps/android/version.properties")
CHANGELOG = Path("apps/android/CHANGELOG.md")
VERSION = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"


def version_parts(version):
    if not re.fullmatch(VERSION, version):
        raise ValueError("Use a version such as 0.1.2, without a v prefix")
    return tuple(int(part) for part in version.split("."))


def read_version(text):
    values = {}
    for line in text.splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        key, separator, value = line.partition("=")
        if not separator or key in values or key not in ("versionName", "versionCode"):
            raise ValueError("Expected one versionName and one versionCode")
        values[key] = value
    if values.keys() != {"versionName", "versionCode"}:
        raise ValueError("Expected one versionName and one versionCode")
    version_parts(values["versionName"])
    if not re.fullmatch(r"[1-9][0-9]*", values["versionCode"]):
        raise ValueError("Invalid Android version code")
    code = int(values["versionCode"])
    if code > 2100000000:
        raise ValueError("Android version code exceeds the Play limit")
    return values["versionName"], code


def tag_version(tag):
    if not tag.startswith("android-v"):
        raise ValueError("Expected an android-vMAJOR.MINOR.PATCH release tag")
    version = tag.removeprefix("android-v")
    version_parts(version)
    return version


def release_notes(text, version):
    version_parts(version)
    headings = list(re.finditer(r"^## \[" + re.escape(version) + r"\](?:[^\S\n].*)?$", text, re.M))
    if len(headings) != 1:
        raise ValueError(f"Changelog needs exactly one ## [{version}] section")
    notes = re.split(r"^## ", text[headings[0].end():], maxsplit=1, flags=re.M)[0].strip()
    if not notes:
        raise ValueError("Release notes must not be empty")
    return notes


def bumped_version(text, requested):
    current, code = read_version(text)
    if version_parts(requested) <= version_parts(current):
        raise ValueError(f"Release version must be greater than {current}")
    if code == 2100000000:
        raise ValueError("Android version code is exhausted")
    return f"versionName={requested}\nversionCode={code + 1}\n"


def git(*arguments):
    return subprocess.check_output(["git", *arguments], cwd=ROOT, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", nargs="?")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--tag")
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()
    text = (ROOT / VERSION_FILE).read_text()
    current, code = read_version(text)
    changelog = (ROOT / CHANGELOG).read_text()
    if args.check:
        if args.version or args.dry_run:
            parser.error("--check cannot be combined with a version or --dry-run")
        if args.tag and tag_version(args.tag) != current:
            raise ValueError("Release tag differs from the tracked Android version")
        notes = release_notes(changelog, current)
        if args.github_output:
            with args.github_output.open("a") as output:
                output.write(f"version_name={current}\nversion_code={code}\n")
                output.write(f"release_tag={args.tag or ''}\n")
        print(notes)
        return
    if not args.version or args.tag or args.github_output:
        parser.error("Provide a version; --tag and --github-output require --check")
    updated = bumped_version(text, args.version)
    release_notes(changelog, args.version)
    if git("status", "--porcelain"):
        raise ValueError("Commit the changelog and all other changes before releasing")
    if git("branch", "--show-current") != "main":
        raise ValueError("Release from main")
    tag = f"android-v{args.version}"
    if git("tag", "--list", tag):
        raise ValueError(f"Release tag already exists: {tag}")
    remote_refs = git("ls-remote", "origin", "refs/heads/main", f"refs/tags/{tag}")
    refs = dict((ref, sha) for sha, ref in (line.split() for line in remote_refs.splitlines()))
    if f"refs/tags/{tag}" in refs:
        raise ValueError(f"Release tag already exists on origin: {tag}")
    if refs.get("refs/heads/main") != git("rev-parse", "HEAD"):
        raise ValueError("Local main must match origin/main before releasing")
    print(f"Release {tag}, Android code {code + 1}: bump, commit, tag, and atomic push")
    if args.dry_run:
        return
    (ROOT / VERSION_FILE).write_text(updated)
    git("add", str(VERSION_FILE))
    git("commit", "-m", f"Release Android v{args.version}")
    git("tag", "-a", tag, "-m", f"Lantern Android {args.version}")
    git("push", "--atomic", "origin", "HEAD:refs/heads/main", f"refs/tags/{tag}")
    print(f"Requested {tag}. CI builds signed artifacts; Play promotion is separate.")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
