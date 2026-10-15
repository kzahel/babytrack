#!/usr/bin/env python3
"""Release transaction regressions; no credentials or network required."""

import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from publish_android_bundle import publish, verify_candidate


BUNDLE = b"signed candidate fixture"
DIGEST = hashlib.sha256(BUNDLE).hexdigest()


class FakePlay:
    def __init__(self, releases=None, uploaded=None, failure=None):
        self.releases = releases or []
        self.uploaded = uploaded or []
        self.failure = failure
        self.calls = []

    def call(self, method, path, body=None, upload=False):
        self.calls.append((method, path, body))
        if self.failure and self.failure in path:
            raise TimeoutError("unknown remote outcome")
        if path == "":
            return {"id": "edit-1"}
        if path.endswith("/tracks"):
            return {"tracks": [{"track": "production", "releases": self.releases}]}
        if path.endswith("/bundles"):
            return {"bundles": self.uploaded}
        if upload:
            return {"versionCode": 2, "sha256": DIGEST}
        return {}


class PublishingTest(unittest.TestCase):
    def run_publish(self, play, status="completed"):
        return publish(play, BUNDLE, DIGEST, 2, "0.1.1", "production", status, "Tracking improvements.")

    def test_validate_precedes_commit_and_receipt_says_submitted(self):
        play = FakePlay()
        self.assertEqual(self.run_publish(play)["status"], "submitted")
        self.assertEqual([p for m, p, _ in play.calls if m == "POST"][-2:],
                         ["/edit-1:validate", "/edit-1:commit?changesNotSentForReview=false"
                          "&changesInReviewBehavior=ERROR_IF_IN_REVIEW"])
        self.assertFalse(any(m == "DELETE" for m, _, _ in play.calls))

    def test_reject_mismatched_existing_bytes_without_track_write(self):
        play = FakePlay(uploaded=[{"versionCode": 2, "sha256": "bad"}])
        with self.assertRaisesRegex(ValueError, "checksum"):
            self.run_publish(play)
        self.assertFalse(any(m == "PUT" for m, _, _ in play.calls))
        self.assertEqual(play.calls[-1][:2], ("DELETE", "/edit-1"))

    def test_refuse_downgrade_and_existing_unrelated_draft(self):
        for release in ({"status": "completed", "versionCodes": ["3"]},
                        {"status": "draft", "versionCodes": ["1"]},
                        {"status": "inProgress", "versionCodes": ["1"]}):
            with self.subTest(release=release):
                play = FakePlay(releases=[release])
                with self.assertRaises(ValueError):
                    self.run_publish(play)
                self.assertFalse(any(m == "PUT" for m, _, _ in play.calls))

    def test_promote_the_same_verified_draft(self):
        play = FakePlay(releases=[{"status": "draft", "versionCodes": ["2"]}],
                        uploaded=[{"versionCode": 2, "sha256": DIGEST}])
        self.assertEqual(self.run_publish(play)["status"], "submitted")
        self.assertFalse(any("uploadType" in p for _, p, _ in play.calls))

    def test_retry_same_release_checks_bytes_and_does_not_commit(self):
        play = FakePlay(releases=[{"status": "completed", "versionCodes": ["2"]}],
                        uploaded=[{"versionCode": 2, "sha256": DIGEST}])
        self.assertEqual(self.run_publish(play)["status"], "unchanged")
        self.assertFalse(any(m == "PUT" or ":commit" in p for m, p, _ in play.calls))

    def test_validation_failure_discards_edit_without_draft_fallback(self):
        play = FakePlay(failure=":validate")
        with self.assertRaises(TimeoutError):
            self.run_publish(play)
        self.assertEqual(play.calls[-1][:2], ("DELETE", "/edit-1"))
        self.assertFalse(any(":commit" in p for _, p, _ in play.calls))

    def test_unknown_commit_does_not_retry_or_delete(self):
        play = FakePlay(failure=":commit")
        with self.assertRaises(TimeoutError):
            self.run_publish(play)
        self.assertEqual(sum(":commit" in p for _, p, _ in play.calls), 1)
        self.assertFalse(any(m == "DELETE" for m, _, _ in play.calls))

    def test_draft_is_not_reported_as_published(self):
        self.assertEqual(self.run_publish(FakePlay(), status="draft")["status"], "draft_saved")

    def test_check_authentication_does_not_upload_or_change_a_track(self):
        play = FakePlay()
        self.assertEqual(self.run_publish(play, status="check")["status"], "access_verified")
        self.assertFalse(any(m == "PUT" or "uploadType" in p or ":commit" in p
                             for m, p, _ in play.calls))
        self.assertEqual(play.calls[-1][:2], ("DELETE", "/edit-1"))

    def test_receipt_and_bytes_must_match_the_requested_ci_run(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            receipt = {"GITHUB_SHA": "a" * 40, "GITHUB_RUN_ID": "123",
                       "RELEASE_CODE": "2", "RELEASE_NAME": "0.1.1", "RELEASE_TAG": "android-v0.1.1"}
            (root / "build.json").write_text(json.dumps(receipt))
            (root / "babytrack.aab").write_bytes(BUNDLE)
            (root / "babytrack.aab.sha256").write_text(f"{DIGEST}  babytrack.aab\n")
            self.assertEqual(verify_candidate(root, "a" * 40, "123", "2", "android-v0.1.1")[1], DIGEST)
            with self.assertRaisesRegex(ValueError, "GITHUB_RUN_ID"):
                verify_candidate(root, "a" * 40, "124", "2", "android-v0.1.1")
            with self.assertRaisesRegex(ValueError, "RELEASE_TAG"):
                verify_candidate(root, "a" * 40, "123", "2", "android-v0.1.2")
            for change in ({"RELEASE_TAG": ""}, {"RELEASE_NAME": "0.1.2"}):
                (root / "build.json").write_text(json.dumps(receipt | change))
                with self.assertRaisesRegex(ValueError, "RELEASE_"):
                    verify_candidate(root, "a" * 40, "123", "2", "android-v0.1.1")
            (root / "build.json").write_text(json.dumps(receipt))
            (root / "babytrack.aab").write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "checksum"):
                verify_candidate(root, "a" * 40, "123", "2", "android-v0.1.1")


if __name__ == "__main__":
    unittest.main()
