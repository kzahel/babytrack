#!/usr/bin/env python3
"""Exercise one local caregiver flow through the installed Android UI."""

from __future__ import annotations

import os
import re
import subprocess
import time
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
APK = ROOT / "apps/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE = "org.babytrack.app"


def command(*args: str) -> str:
    result = subprocess.run(args, check=True, capture_output=True, text=True)
    return result.stdout


def serial() -> str:
    chosen = os.environ.get("ANDROID_SERIAL")
    if chosen:
        return chosen
    devices = [
        line.split()[0]
        for line in command("adb", "devices").splitlines()[1:]
        if line.endswith("\tdevice")
    ]
    if len(devices) != 1:
        raise RuntimeError("Set ANDROID_SERIAL when more than one device is attached")
    return devices[0]


def adb(target: str, *args: str) -> str:
    return command("adb", "-s", target, *args)


def nodes(target: str) -> list[ET.Element]:
    adb(target, "shell", "uiautomator", "dump", "/sdcard/babytrack-ui.xml")
    return list(ET.fromstring(adb(target, "exec-out", "cat", "/sdcard/babytrack-ui.xml")).iter("node"))


def find(target: str, label: str, *, scroll: bool = False, occurrence: int = 0) -> ET.Element:
    deadline = time.monotonic() + 20
    scroll_count = 0
    size = re.search(r"(\d+)x(\d+)", adb(target, "shell", "wm", "size"))
    if size is None:
        raise RuntimeError("Android display size unavailable")
    width, height = map(int, size.groups())
    while time.monotonic() < deadline:
        matches = [node for node in nodes(target) if node.attrib.get("text") == label]
        if len(matches) > occurrence:
            return matches[occurrence]
        if scroll and scroll_count < 6:
            adb(
                target, "shell", "input", "swipe", str(width // 2),
                str(height * 4 // 5), str(width // 2), str(height * 3 // 10), "360",
            )
            scroll_count += 1
        else:
            time.sleep(0.4)
    raise AssertionError(f"Android UI did not show {label!r}")


def tap(target: str, label: str, *, scroll: bool = False, occurrence: int = 0) -> None:
    bounds = find(target, label, scroll=scroll, occurrence=occurrence).attrib["bounds"]
    x1, y1, x2, y2 = map(int, re.findall(r"\d+", bounds))
    adb(target, "shell", "input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))


def scroll_up(target: str, times: int = 4) -> None:
    size = re.search(r"(\d+)x(\d+)", adb(target, "shell", "wm", "size"))
    if size is None:
        raise RuntimeError("Android display size unavailable")
    width, height = map(int, size.groups())
    for _ in range(times):
        adb(target, "shell", "input", "swipe", str(width // 2),
            str(height * 3 // 10), str(width // 2), str(height * 4 // 5), "300")


def main() -> None:
    target = serial()
    if adb(target, "shell", "getprop", "ro.kernel.qemu").strip() != "1":
        raise RuntimeError("This destructive smoke check runs only on an emulator")
    if not APK.exists():
        raise RuntimeError(f"Build the debug APK first: {APK}")
    adb(target, "install", "-r", str(APK))
    adb(target, "shell", "pm", "clear", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Join a Family")
    tap(target, "New Family")
    find(target, "Sharing controls")
    tap(target, "Child’s name", scroll=True)
    adb(target, "shell", "input", "text", "UITestChild")
    adb(target, "shell", "input", "keyevent", "4")
    tap(target, "Add child", scroll=True)
    find(target, "UITestChild", scroll=True)
    tap(target, "Wet", scroll=True)
    find(target, "Diaper · Wet", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "UITestChild", scroll=True)
    find(target, "Diaper · Wet", scroll=True)
    tap(target, "Delete entry", scroll=True)
    find(target, "Remove this entry from the Family timeline? Shared devices receive the change when they sync.")
    tap(target, "Delete")
    find(target, "No entries yet.", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "UITestChild", scroll=True)
    find(target, "No entries yet.", scroll=True)
    scroll_up(target)
    tap(target, "What happened?", scroll=True)
    adb(target, "shell", "input", "text", "Before")
    adb(target, "shell", "input", "keyevent", "4")
    tap(target, "Save note", scroll=True)
    find(target, "Note · Before", scroll=True)
    tap(target, "Edit note", scroll=True)
    tap(target, "Before")
    adb(target, "shell", "input", "text", "After")
    adb(target, "shell", "input", "keyevent", "4")
    tap(target, "Save changes")
    find(target, "Note · BeforeAfter", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Note · BeforeAfter", scroll=True)
    scroll_up(target, 12)
    tap(target, "Amount (mL)", scroll=True)
    adb(target, "shell", "input", "text", "90")
    adb(target, "shell", "input", "keyevent", "4")
    tap(target, "Log bottle", scroll=True, occurrence=1)
    find(target, "Bottle · 90 mL", scroll=True)
    tap(target, "Edit bottle amount", scroll=True)
    tap(target, "Amount (mL)")
    adb(target, "shell", "input", "keyevent", "123")
    adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "text", "120")
    adb(target, "shell", "input", "keyevent", "4")
    tap(target, "Save changes")
    find(target, "Bottle · 120 mL", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    tap(target, "Minutes on selected side", scroll=True)
    adb(target, "shell", "input", "text", "5")
    adb(target, "shell", "input", "keyevent", "4")
    tap(target, "Add segment", scroll=True)
    tap(target, "Right", scroll=True)
    tap(target, "Minutes on selected side", scroll=True)
    adb(target, "shell", "input", "text", "8")
    adb(target, "shell", "input", "keyevent", "4")
    tap(target, "Save breast feed", scroll=True)
    find(target, "Breast · Left 5 min → Right 8 min", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Breast · Left 5 min → Right 8 min", scroll=True)
    scroll_up(target)
    find(target, "Bottle · 120 mL", scroll=True)
    print("Android UI Family, diaper deletion, note and bottle edits, breast segments, and restart: OK")


if __name__ == "__main__":
    main()
