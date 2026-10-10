"""Regress bounded commands and preservation of the original UI failure."""

import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from unittest.mock import patch

import android_ui


class HarnessDiagnosticsTest(unittest.TestCase):
    def test_killed_dump_retries_without_reading_stale_xml(self):
        calls = []
        failures = 0

        def adb(_target, *args, **kwargs):
            nonlocal failures
            calls.append(args)
            if args[:3] == ('shell', 'uiautomator', 'dump'):
                if failures == 0:
                    failures += 1
                    raise subprocess.CalledProcessError(137, args)
                return 'UI hierarchy dumped to: /sdcard/babytrack-ui.xml'
            if args[:2] == ('exec-out', 'cat'):
                return '<hierarchy><node text="fresh screen" /></hierarchy>'
            return ''

        with patch.object(android_ui, 'adb', side_effect=adb), \
                patch.object(android_ui.time, 'sleep'), patch.object(android_ui, 'stage'):
            root = android_ui.nodes('emulator-5554')
        self.assertEqual(root[0].attrib['text'], 'fresh screen')
        self.assertEqual([args[:3] for args in calls], [
            ('shell', 'rm', '-f'), ('shell', 'uiautomator', 'dump'),
            ('shell', 'rm', '-f'), ('shell', 'uiautomator', 'dump'),
            ('exec-out', 'cat', '/sdcard/babytrack-ui.xml'),
        ])

    def test_persistent_killed_dump_fails_after_three_attempts(self):
        failure = subprocess.CalledProcessError(137, 'uiautomator dump')

        def adb(_target, *args, **kwargs):
            if args[:3] == ('shell', 'uiautomator', 'dump'):
                raise failure
            return ''

        with patch.object(android_ui, 'adb', side_effect=adb) as mocked, \
                patch.object(android_ui.time, 'sleep'), patch.object(android_ui, 'stage'):
            with self.assertRaises(subprocess.CalledProcessError) as raised:
                android_ui.nodes('emulator-5554')
        self.assertIs(raised.exception, failure)
        self.assertEqual(mocked.call_count, 6)

    def test_other_dump_errors_fail_immediately(self):
        failure = subprocess.CalledProcessError(1, 'uiautomator dump')
        with patch.object(android_ui, 'adb', side_effect=['', failure]) as mocked:
            with self.assertRaises(subprocess.CalledProcessError) as raised:
                android_ui.nodes('emulator-5554')
        self.assertIs(raised.exception, failure)
        self.assertEqual(mocked.call_count, 2)

    def test_compact_navigation_is_not_an_expanded_entry_action(self):
        # CI's 320x640 viewport puts the Family touch target 36 px below
        # the row, while its text label is another 48 px further down.
        collapsed = ET.fromstring('''<hierarchy><node bounds="[0,0][320,640]">
          <node clickable="true" bounds="[28,420][292,476]">
            <node text="Diaper · Wet" bounds="[84,438][163,458]" />
          </node>
          <node clickable="true" bounds="[0,512][102,592]">
            <node text="Today" bounds="[33,560][69,576]" />
          </node>
          <node clickable="true" bounds="[219,512][320,592]">
            <node text="Family" bounds="[250,560][289,576]" />
          </node>
        </node></hierarchy>''')
        expanded = ET.fromstring(ET.tostring(collapsed))
        # Once opened/scrolled, a real entry action is visible below the row.
        expanded.find('.//node[@clickable="true"]').set('bounds', '[28,300][292,356]')
        ET.SubElement(expanded[0], 'node', clickable='true', text='Edit diaper type',
                      bounds='[72,360][220,408]')
        current = collapsed
        taps = []

        def adb(_target, *args):
            nonlocal current
            if args == ('shell', 'wm', 'size'):
                return 'Physical size: 320x640'
            if args[:3] == ('shell', 'input', 'tap'):
                taps.append(args)
                current = expanded
            return ''

        with patch.object(android_ui, 'find'), \
                patch.object(android_ui, 'nodes', side_effect=lambda _: current), \
                patch.object(android_ui, 'adb', side_effect=adb), \
                patch.object(android_ui.time, 'sleep'), patch.object(android_ui, 'stage'):
            android_ui.open_entry_actions('emulator-5554', 'Diaper · Wet')
            self.assertEqual(len(taps), 1)
            # Calling it again must leave the already-open row expanded.
            android_ui.open_entry_actions('emulator-5554', 'Diaper · Wet')
            self.assertEqual(len(taps), 1)

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
