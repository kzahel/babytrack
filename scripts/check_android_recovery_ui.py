#!/usr/bin/env python3
"""Save through Android's document picker, reinstall, and restore the file."""

from __future__ import annotations

import argparse
import re
import subprocess
import tempfile
from pathlib import Path

from android_ui import (
    APK, PACKAGE, adb, dismiss_keyboard, find, nodes, scroll_up, serial, tap, run_scenario, stage,
)


FILENAME = "babytrack-backup.cbor"
DOWNLOAD = f"/storage/emulated/0/Download/{FILENAME}"
PROTECTED_FILENAME = "babytrack-backup.btbk"
PROTECTED_DOWNLOAD = f"/storage/emulated/0/Download/{PROTECTED_FILENAME}"
CORRUPT_FILENAME = "babytrack-corrupt.cbor"
CORRUPT_DOWNLOAD = f"/storage/emulated/0/Download/{CORRUPT_FILENAME}"
TEST_PASSWORD = "TestPassphrase42"


def reinstall(target: str) -> None:
    stage("Recovery: fresh installation")
    adb(target, "uninstall", PACKAGE)
    adb(target, "install", str(APK))
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")


def main() -> None:
    target = serial()
    if adb(target, "shell", "getprop", "ro.kernel.qemu").strip() != "1":
        raise RuntimeError("This destructive recovery check runs only on an emulator")
    if not APK.exists():
        raise RuntimeError(f"Build the debug APK first: {APK}")
    adb(target, "install", "-r", str(APK))
    adb(target, "shell", "pm", "clear", PACKAGE)
    downloads = adb(target, "shell", "ls", "/storage/emulated/0/Download").splitlines()
    for name in (FILENAME, PROTECTED_FILENAME, CORRUPT_FILENAME):
        if name in downloads:
            adb(target, "shell", "rm", f"/storage/emulated/0/Download/{name}")
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")

    tap(target, "Add your child")
    tap(target, "Name or nickname", scroll=True)
    adb(target, "shell", "input", "text", "RecoveryChild")
    dismiss_keyboard(target)
    tap(target, "Start tracking", scroll=True)
    find(target, "Family 1 · RecoveryChild")
    tap(target, "Add activity", scroll=True, actionable=True)
    tap(target, "Add note", scroll=True, actionable=True)
    tap(target, "What happened?", scroll=True)
    adb(target, "shell", "input", "text", "RecoveryMarker")
    dismiss_keyboard(target)
    tap(target, "Save note", scroll=True)
    tap(target, "History", actionable=True)
    find(target, "Note · RecoveryMarker", scroll=True)

    tap(target, "Family", actionable=True)
    tap(target, "Data and backups", scroll=True)
    stage("Recovery: save backup")
    tap(target, "Save backup", scroll=True)
    tap(target, "SAVE")
    find(target, "Last completed file save:", scroll=True, contains=True)
    size = int(adb(target, "shell", "stat", "-c", "%s", DOWNLOAD).strip())
    if size < 100:
        raise AssertionError(f"Saved backup was unexpectedly short: {size} bytes")
    readable = subprocess.run(
        ["adb", "-s", target, "exec-out", "cat", DOWNLOAD],
        check=True, capture_output=True, timeout=60,
    ).stdout
    with tempfile.TemporaryDirectory() as scratch:
        corrupt = Path(scratch) / CORRUPT_FILENAME
        corrupt.write_bytes(readable[:len(readable) // 2])
        subprocess.run(["adb", "-s", target, "push", str(corrupt), CORRUPT_DOWNLOAD],
                       check=True, capture_output=True, timeout=60)
    tap(target, "Today", actionable=True)
    tap(target, "Add activity", scroll=True, actionable=True)
    tap(target, "Add note", scroll=True, actionable=True)
    tap(target, "What happened?", scroll=True)
    adb(target, "shell", "input", "text", "AfterBackupMarker")
    dismiss_keyboard(target)
    tap(target, "Save note", scroll=True)
    tap(target, "Family", actionable=True)
    find(target, "Changes since that save are not in that file.", scroll=True)

    reinstall(target)
    find(target, "Add your child")
    stage("Recovery: open saved file")
    tap(target, "Restore backup")
    tap(target, CORRUPT_FILENAME)
    find(target, "This backup file is damaged or unsupported. No Family was restored.")
    if any(node.attrib.get("text", "").startswith("Family 1")
           for node in nodes(target).iter("node")):
        raise AssertionError("Damaged backup created a Family")
    stage("Recovery: open saved file")
    tap(target, "Restore backup")
    tap(target, FILENAME)
    find(target, "File saved at ", scroll=True, contains=True)
    tap(target, "Restore as new Family", scroll=True)
    tap(target, "Family", actionable=True)
    find(target, "Restored from a file saved at ", scroll=True, contains=True)
    find(target, "Family 1 · RecoveryChild")
    tap(target, "History", actionable=True)
    display = re.search(r"(\d+)x(\d+)", adb(target, "shell", "wm", "size"))
    if display is None:
        raise RuntimeError("Android display size unavailable")
    width, height = map(int, display.groups())
    seen: set[str] = set()
    for _ in range(8):
        seen.update(node.attrib["text"] for node in nodes(target).iter("node")
                    if node.attrib.get("text"))
        adb(target, "shell", "input", "swipe", str(width // 2), str(height * 4 // 5),
            str(width // 2), str(height * 3 // 10), "300")
    if "Note · RecoveryMarker" not in seen or "Note · AfterBackupMarker" in seen:
        raise AssertionError(f"Restored timeline did not match file point: {seen!r}")
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Family 1 · RecoveryChild")
    tap(target, "History", actionable=True)
    find(target, "Note · RecoveryMarker", scroll=True)

    tap(target, "Family", actionable=True)
    tap(target, "Data and backups", scroll=True)
    tap(target, "Protect with password", scroll=True)
    tap(target, "Backup password", scroll=True)
    adb(target, "shell", "input", "text", TEST_PASSWORD)
    dismiss_keyboard(target)
    stage("Recovery: save backup")
    tap(target, "Save backup", scroll=True)
    tap(target, "SAVE")
    find(target, "Last completed file save:", scroll=True, contains=True)
    protected_size = int(adb(target, "shell", "stat", "-c", "%s", PROTECTED_DOWNLOAD).strip())
    if protected_size < 100:
        raise AssertionError(f"Protected backup was unexpectedly short: {protected_size} bytes")

    reinstall(target)
    find(target, "Add your child")
    stage("Recovery: open saved file")
    tap(target, "Restore backup")
    tap(target, PROTECTED_FILENAME)
    find(target, "Password for protected backup")
    tap(target, "Password for protected backup", scroll=True)
    adb(target, "shell", "input", "text", "WrongPassphrase")
    dismiss_keyboard(target)
    tap(target, "Check protected backup", scroll=True)
    find(target, "Wrong password or damaged backup file.", scroll=True)
    if any(node.attrib.get("text", "").startswith("Family 1")
           for node in nodes(target).iter("node")):
        raise AssertionError("Wrong password created a Family")
    tap(target, "Password for protected backup", scroll=True)
    for _ in range(20):
        adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "text", TEST_PASSWORD)
    dismiss_keyboard(target)
    tap(target, "Check protected backup", scroll=True)
    find(target, "File saved at ", scroll=True, contains=True)
    tap(target, "Restore as new Family", scroll=True)
    find(target, "Family 1 · RecoveryChild")
    tap(target, "History", actionable=True)
    find(target, "Note · RecoveryMarker", scroll=True)
    print("Android readable/protected backups, damaged-file denial, fresh-install restore, and restart: OK")


if __name__ == "__main__":
    argparse.ArgumentParser(description=__doc__).parse_args()
    run_scenario("file-recovery", main)
