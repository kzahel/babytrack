#!/usr/bin/env python3
"""Upload a verified CI bundle through a transactional Google Play edit."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen


PACKAGE = "org.babytrack.app"
API = "https://androidpublisher.googleapis.com"


def verify_candidate(directory, source, run_id, version_code):
    receipt = json.loads((directory / "build.json").read_text())
    if not re.fullmatch(r"[a-f0-9]{40}", source):
        raise ValueError("Invalid source revision")
    if not 1 <= int(version_code) <= 2100000000:
        raise ValueError("Invalid version code")
    for key, expected in (("GITHUB_SHA", source), ("GITHUB_RUN_ID", run_id),
                          ("RELEASE_CODE", version_code)):
        if receipt.get(key) != str(expected):
            raise ValueError(f"Candidate receipt differs: {key}")
    bundle = (directory / "babytrack.aab").read_bytes()
    digest = hashlib.sha256(bundle).hexdigest()
    if (directory / "babytrack.aab.sha256").read_text().split() != [digest, "babytrack.aab"]:
        raise ValueError("Candidate checksum differs")
    return bundle, digest, receipt["RELEASE_NAME"]


class Play:
    def __init__(self, token):
        if not token:
            raise ValueError("PLAY_ACCESS_TOKEN is required")
        self.token = token

    def call(self, method, path, body=None, upload=False):
        prefix = "/upload" if upload else ""
        url = f"{API}{prefix}/androidpublisher/v3/applications/{PACKAGE}/edits{path}"
        data = body if upload else (None if body is None else json.dumps(body).encode())
        request = Request(url, data=data, method=method, headers={
            "Authorization": f"Bearer {self.token}",
            "Content-Type": "application/octet-stream" if upload else "application/json",
        })
        try:
            with urlopen(request, timeout=180 if upload else 30) as response:
                content = response.read()
                return json.loads(content) if content else {}
        except HTTPError as error:
            # Never print arbitrary response bodies or authentication headers.
            raise RuntimeError(f"Play API {method} {path}: HTTP {error.code}") from None


def publish(play, bundle, digest, version, name, track, status, notes):
    if track not in ("internal", "production") or status not in ("check", "draft", "completed"):
        raise ValueError("Unsupported track or release status")
    if not notes.strip() or len(notes) > 500:
        raise ValueError("Release notes must contain 1–500 characters")
    edit = play.call("POST", "", {})
    path = "/" + quote(edit["id"], safe="")
    commit_attempted = False
    try:
        tracks = play.call("GET", f"{path}/tracks").get("tracks", [])
        if status == "check":
            play.call("GET", f"{path}/bundles")
            return {"status": "access_verified", "track": track, "versionCode": version}
        current = next((entry for entry in tracks if entry["track"] == track), {})
        releases = current.get("releases", [])
        exact = next((r for r in releases if r.get("versionCodes") == [str(version)]
                      and r.get("status") == status), None)
        bundles = play.call("GET", f"{path}/bundles").get("bundles", [])
        uploaded = next((b for b in bundles if str(b["versionCode"]) == str(version)), None)
        if uploaded is None:
            uploaded = play.call("POST", f"{path}/bundles?uploadType=media", bundle, upload=True)
        if str(uploaded["versionCode"]) != str(version) or uploaded["sha256"] != digest:
            raise ValueError("Play bundle version/checksum differs from candidate")
        if exact:
            return {"status": "unchanged", "track": track, "versionCode": version}
        own_draft = [r for r in releases if r.get("status") == "draft"
                     and r.get("versionCodes") == [str(version)]]
        if any(r.get("status") in ("inProgress", "halted") or
               (r.get("status") == "draft" and r not in own_draft) for r in releases):
            raise ValueError("Resolve the existing rollout or draft in Play Console first")
        latest = max([0] + [int(code) for r in releases for code in r.get("versionCodes", [])])
        if version < latest or (version == latest and not own_draft):
            raise ValueError("Refusing to replace a newer or equal release")
        play.call("PUT", f"{path}/tracks/{track}", {"track": track, "releases": [{
            "name": name, "versionCodes": [str(version)], "status": status,
            "releaseNotes": [{"language": "en-US", "text": notes}],
        }]})
        play.call("POST", f"{path}:validate", {})
        # A timeout here has an unknown outcome. Reconcile in Console before retrying;
        # do not silently downgrade to draft or suppress review errors.
        commit_attempted = True
        play.call("POST", f"{path}:commit?changesNotSentForReview=false", {})
        return {"status": "draft_saved" if status == "draft" else "submitted",
                "track": track, "versionCode": version, "sha256": digest,
                "editId": edit["id"]}
    finally:
        if not commit_attempted:
            try:
                play.call("DELETE", path)
            except Exception:
                print("Could not discard uncommitted edit; reconcile in Console", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--source", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--version-code", required=True)
    parser.add_argument("--track", choices=["internal", "production"], required=True)
    parser.add_argument("--status", choices=["check", "draft", "completed"], default="check")
    parser.add_argument("--notes-file", type=Path, required=True)
    args = parser.parse_args()
    bundle, digest, name = verify_candidate(args.directory, args.source, args.run_id, args.version_code)
    result = publish(Play(os.environ.get("PLAY_ACCESS_TOKEN")), bundle, digest,
                     int(args.version_code), name, args.track, args.status,
                     args.notes_file.read_text().strip())
    result.update(source=args.source, buildRunId=args.run_id)
    (args.directory / "publication.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))
    print("Check Play Console review/publication status; submission does not prove availability.")


if __name__ == "__main__":
    main()
