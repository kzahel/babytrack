"""Regress bounded commands and preservation of the original UI failure."""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import android_ui


class HarnessDiagnosticsTest(unittest.TestCase):
    def test_a_blocked_subprocess_times_out(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            android_ui.command(sys.executable, "-c", "import time; time.sleep(10)", timeout=0.1)

    def test_diagnostic_failure_does_not_mask_the_original_scenario_failure(self):
        original = AssertionError("missing fixture control")
        def fail():
            raise original
        with patch.object(android_ui, "serial", return_value="emulator-5554"), \
                patch.object(android_ui, "adb", return_value="1"), \
                patch.object(android_ui, "capture_failure", side_effect=OSError("unavailable")), \
                patch.object(android_ui, "stage"):
            with self.assertRaises(AssertionError) as raised:
                android_ui.run_scenario("fixture", fail)
        self.assertIs(raised.exception, original)

    def test_partial_diagnostics_preserve_the_available_artifacts(self):
        timeout = subprocess.TimeoutExpired("adb", 10)
        with tempfile.TemporaryDirectory() as scratch, \
                patch.dict(android_ui.os.environ, {"BABYTRACK_ANDROID_DIAGNOSTICS_DIR": scratch}), \
                patch.object(android_ui.subprocess, "run", side_effect=[
                    timeout, subprocess.CompletedProcess([], 0, b"fixture log", b""),
                    subprocess.CompletedProcess([], 0, b"<hierarchy />", b""),
                ]):
            output = android_ui.capture_failure("emulator-5554", "fixture")
            self.assertEqual((output / "logcat.txt").read_bytes(), b"fixture log")
            self.assertEqual((output / "last-ui.xml").read_text(), "<hierarchy />")
            self.assertIn("timed out", (output / "screen.png.error.txt").read_text())


if __name__ == "__main__":
    unittest.main()
