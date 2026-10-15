#!/usr/bin/env python3
"""Versioned release and publication authority regressions, using local Git."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from release_android import bumped_version, read_version, release_notes, tag_version
from verify_android_release import has_source_checks, verify_build


SCRIPTS = Path(__file__).resolve().parent
REPOSITORY = "owner/babytrack"
TAG = "android-v0.1.2"
SOURCE = "a" * 40


def build_run():
    return {"head_repository": {"full_name": REPOSITORY}, "event": "push",
            "head_branch": TAG, "head_sha": SOURCE, "status": "completed",
            "conclusion": "success", "path": ".github/workflows/android-internal.yml"}


def source_checks():
    return dict(build_run(), head_branch="main", path=".github/workflows/scaffold.yml")


class ReleaseAuthorityTest(unittest.TestCase):
    def test_tag_and_versions_reject_ambiguous_or_unsafe_input(self):
        original = "versionName=0.1.1\nversionCode=4\n"
        self.assertEqual(read_version(bumped_version(original, "0.1.2")), ("0.1.2", 5))
        for version in ("v0.1.2", "01.1.2", "0.1.2\n", "0.1.2;echo bad", "0.1.1", "0.1.0"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                bumped_version(original, version)
        for text in (original * 2, original.replace("=4", "=0"), original.replace("=4", "=2100000001")):
            with self.assertRaises(ValueError):
                read_version(text)
        with self.assertRaises(ValueError):
            bumped_version(original.replace("=4", "=2100000000"), "0.1.2")
        for tag in ("v0.1.2", "android-v0.1.2/extra", "android-v0.1.2\n", "main"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                tag_version(tag)

    def test_changelog_requires_exact_unique_nonempty_section(self):
        self.assertEqual(release_notes("## [0.1.2]\nNew\n## [0.1.1]\nOld", "0.1.2"), "New")
        for text in ("## [0x1x2]\nWrong", "## [0.1.2]\n", "## [0.1.2]\nA\n## [0.1.2]\nB"):
            with self.assertRaises(ValueError):
                release_notes(text, "0.1.2")

    def test_only_matching_successful_tag_push_build_can_be_promoted(self):
        verify_build(build_run(), REPOSITORY, TAG, SOURCE)
        for key, value in (("head_branch", "main"), ("event", "workflow_dispatch"),
                           ("head_sha", "b" * 40), ("conclusion", "failure"),
                           ("path", ".github/workflows/scaffold.yml"),
                           ("head_repository", {"full_name": "someone/fork"})):
            with self.subTest(key=key), self.assertRaises(ValueError):
                verify_build(dict(build_run(), **{key: value}), REPOSITORY, TAG, SOURCE)

    def test_ci_must_cover_exact_tagged_source_on_main(self):
        self.assertTrue(has_source_checks([source_checks()], REPOSITORY, SOURCE))
        for key, value in (("head_branch", TAG), ("head_sha", "b" * 40),
                           ("conclusion", "failure"), ("event", "pull_request"),
                           ("path", ".github/workflows/android-internal.yml"),
                           ("head_repository", {"full_name": "someone/fork"})):
            with self.subTest(key=key):
                self.assertFalse(has_source_checks([dict(source_checks(), **{key: value})], REPOSITORY, SOURCE))


class ReleaseGitTest(unittest.TestCase):
    def test_prepare_tag_push_and_verify_with_local_remote(self):
        with tempfile.TemporaryDirectory(prefix="babytrack-release-test-") as directory:
            root = Path(directory) / "checkout"
            root.mkdir()
            env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_TERMINAL_PROMPT="0")

            def run(*arguments, check=True, **extra_env):
                return subprocess.run(arguments, cwd=root, env=env | extra_env,
                                      capture_output=True, text=True, check=check)

            run("git", "init", "-b", "main")
            run("git", "config", "user.name", "Release Tester")
            run("git", "config", "user.email", "release@localhost")
            remote = str(Path(directory) / "remote.git")
            run("git", "init", "--bare", remote)
            run("git", "remote", "add", "origin", remote)
            (root / "scripts").mkdir()
            for name in ("release_android.py", "verify_android_release.py"):
                shutil.copy(SCRIPTS / name, root / "scripts" / name)
            android = root / "apps/android"
            android.mkdir(parents=True)
            (android / "version.properties").write_text("versionName=0.1.1\nversionCode=4\n")
            (android / "CHANGELOG.md").write_text("## [0.1.2]\n\nRelease fixture\n")
            run("git", "add", ".")
            run("git", "commit", "-m", "Initial fixture")
            run("git", "push", "origin", "main")
            before = run("git", "rev-parse", "HEAD").stdout
            run("python3", "scripts/release_android.py", "0.1.2", "--dry-run")
            self.assertEqual(run("git", "status", "--porcelain").stdout, "")
            self.assertEqual(run("git", "rev-parse", "HEAD").stdout, before)
            (root / "untracked").touch()
            self.assertNotEqual(run("python3", "scripts/release_android.py", "0.1.2", check=False).returncode, 0)
            (root / "untracked").unlink()
            run("python3", "scripts/release_android.py", "0.1.2")
            source = run("git", "rev-parse", "HEAD").stdout.strip()
            self.assertEqual(read_version((android / "version.properties").read_text()), ("0.1.2", 5))
            self.assertEqual(run("git", "rev-parse", f"{TAG}^{{commit}}").stdout.strip(), source)
            refs = run("git", "ls-remote", "origin").stdout
            self.assertIn(f"{source}\trefs/heads/main", refs)
            self.assertIn(f"{source}\trefs/tags/{TAG}^{{}}", refs)
            self.assertNotEqual(run("python3", "scripts/release_android.py", "--check", "--tag",
                                    "android-v0.1.3", check=False).returncode, 0)
            # A fake API serves run metadata only; tag resolution/ancestry are real Git.
            api = Path(directory) / "bin"
            api.mkdir()
            fake_gh = api / "gh"
            fake_gh.write_text("#!/usr/bin/env python3\nimport os\nprint(os.environ['FAKE_RUN'] if 'actions/runs/' in __import__('sys').argv[-1] else os.environ['FAKE_CHECKS'])\n")
            fake_gh.chmod(0o755)
            output = Path(directory) / "outputs"
            valid_build = dict(build_run(), head_sha=source)
            checks = {"workflow_runs": [dict(source_checks(), head_sha=source)]}
            verification_env = dict(PATH=str(api) + os.pathsep + env["PATH"], RELEASE_TAG=TAG,
                                    BUILD_RUN="123", VERSION_CODE="5", GITHUB_REPOSITORY=REPOSITORY,
                                    GITHUB_OUTPUT=str(output), FAKE_RUN=json.dumps(valid_build),
                                    FAKE_CHECKS=json.dumps(checks))
            run("python3", "scripts/verify_android_release.py", **verification_env)
            self.assertEqual(output.read_text(), f"source={source}\n")
            for changes in ({"VERSION_CODE": "4"}, {"FAKE_CHECKS": '{"workflow_runs":[]}'},
                            {"FAKE_RUN": json.dumps(dict(valid_build, event="workflow_dispatch"))}):
                result = run("python3", "scripts/verify_android_release.py", check=False,
                             **(verification_env | changes))
                self.assertNotEqual(result.returncode, 0, result.stdout)
            run("git", "checkout", "-b", "unreleased")
            (android / "version.properties").write_text("versionName=0.1.3\nversionCode=6\n")
            run("git", "add", ".")
            run("git", "commit", "-m", "Unmerged candidate")
            side_source = run("git", "rev-parse", "HEAD").stdout.strip()
            run("git", "tag", "-a", "android-v0.1.3", "-m", "Unmerged tag")
            run("git", "push", "origin", "refs/tags/android-v0.1.3")
            unmerged = verification_env | {
                "RELEASE_TAG": "android-v0.1.3", "VERSION_CODE": "6",
                "FAKE_RUN": json.dumps(dict(valid_build, head_branch="android-v0.1.3", head_sha=side_source)),
                "FAKE_CHECKS": json.dumps({"workflow_runs": [dict(source_checks(), head_sha=side_source)]}),
            }
            self.assertNotEqual(run("python3", "scripts/verify_android_release.py", check=False,
                                    **unmerged).returncode, 0)


if __name__ == "__main__":
    unittest.main()
