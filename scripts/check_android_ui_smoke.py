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


def nodes(target: str) -> ET.Element:
    adb(target, "shell", "uiautomator", "dump", "/sdcard/babytrack-ui.xml")
    return ET.fromstring(adb(target, "exec-out", "cat", "/sdcard/babytrack-ui.xml"))


def find(target: str, label: str, *, scroll: bool = False, occurrence: int = 0,
         contains: bool = False, actionable: bool = False) -> ET.Element:
    deadline = time.monotonic() + (100 if scroll else 20)
    scroll_count = 0
    visible: list[str] = []
    size = re.search(r"(\d+)x(\d+)", adb(target, "shell", "wm", "size"))
    if size is None:
        raise RuntimeError("Android display size unavailable")
    width, height = map(int, size.groups())
    while time.monotonic() < deadline:
        root = nodes(target)
        visible = [node.attrib["text"] for node in root.iter("node") if node.attrib.get("text")]
        matches = [
            node for node in root.iter("node")
            if (label in node.attrib.get("text", "") if contains else node.attrib.get("text") == label)
        ]
        if actionable:
            parents = {child: parent for parent in root.iter() for child in parent}
            actions = []
            for node in matches:
                while node.attrib.get("clickable") != "true" and node in parents:
                    node = parents[node]
                if node.attrib.get("clickable") == "true":
                    actions.append(node)
            matches = actions
        if len(matches) > occurrence:
            return matches[occurrence]
        if scroll and scroll_count < 50:
            down = scroll_count < 25
            adb(
                target, "shell", "input", "swipe", str(width // 2),
                str(height * 4 // 5 if down else height * 3 // 10), str(width // 2),
                str(height * 3 // 10 if down else height * 4 // 5), "360",
            )
            scroll_count += 1
        else:
            time.sleep(0.4)
    raise AssertionError(f"Android UI did not show {label!r}; last visible text: {visible!r}")


def tap(target: str, label: str, *, scroll: bool = False, occurrence: int = 0,
        actionable: bool = False) -> None:
    bounds = find(target, label, scroll=scroll, occurrence=occurrence,
                  actionable=actionable).attrib["bounds"]
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


def dismiss_keyboard(target: str) -> None:
    if "mInputShown=true" in adb(target, "shell", "dumpsys", "input_method"):
        adb(target, "shell", "input", "keyevent", "4")


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
    dismiss_keyboard(target)
    tap(target, "Add child", scroll=True)
    find(target, "UITestChild", scroll=True)
    tap(target, "View timeline", scroll=True)
    find(target, "No entries yet.")
    scroll_up(target, 12)
    tap(target, "Wet", scroll=True)
    find(target, "Diaper · Wet", scroll=True)
    tap(target, "Edit diaper type", scroll=True)
    tap(target, "Dirty")
    tap(target, "Save changes")
    find(target, "Diaper · Dirty", scroll=True)
    scroll_up(target, 2)
    tap(target, "Feeds", scroll=True)
    find(target, "No entries in this view.", scroll=True)
    scroll_up(target, 2)
    tap(target, "All", scroll=True)
    find(target, "Diaper · Dirty", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "UITestChild", scroll=True)
    tap(target, "View timeline", scroll=True)
    find(target, "Diaper · Dirty")
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
    dismiss_keyboard(target)
    tap(target, "Save note", scroll=True)
    find(target, "Note · Before", scroll=True)
    tap(target, "Edit note", scroll=True)
    tap(target, "Before")
    adb(target, "shell", "input", "text", "After")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Note · BeforeAfter", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Note · BeforeAfter", scroll=True)
    scroll_up(target, 12)
    tap(target, "Dry", scroll=True)
    find(target, "Diaper · Dry", scroll=True)
    scroll_up(target, 12)
    tap(target, "Breast milk", scroll=True)
    tap(target, "Amount (mL)", scroll=True)
    adb(target, "shell", "input", "text", "90")
    dismiss_keyboard(target)
    tap(target, "Log bottle", scroll=True, actionable=True)
    find(target, "Bottle · 90 mL · Breast milk", scroll=True)
    tap(target, "Edit bottle", scroll=True)
    tap(target, "Formula")
    tap(target, "Amount (mL)")
    adb(target, "shell", "input", "keyevent", "123")
    adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "text", "120")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Bottle · 120 mL · Formula", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    tap(target, "Minutes on selected side", scroll=True)
    adb(target, "shell", "input", "text", "5")
    dismiss_keyboard(target)
    tap(target, "Add segment", scroll=True)
    scroll_up(target, 12)
    tap(target, "Right", scroll=True)
    tap(target, "Minutes on selected side", scroll=True)
    adb(target, "shell", "input", "text", "8")
    dismiss_keyboard(target)
    tap(target, "Save breast feed", scroll=True)
    find(target, "Breast · Left 5 min → Right 8 min", scroll=True)
    tap(target, "Edit breast feed", scroll=True)
    tap(target, "Right")
    tap(target, "Save changes")
    find(target, "Breast · Right 5 min → Right 8 min", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Breast · Right 5 min → Right 8 min", scroll=True)
    scroll_up(target)
    find(target, "Bottle · 120 mL · Formula", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    tap(target, "Rename child", scroll=True)
    tap(target, "New child name", scroll=True)
    adb(target, "shell", "input", "text", "Renamed")
    dismiss_keyboard(target)
    find(target, "Renamed", contains=True)
    tap(target, "Save changes", scroll=True)
    scroll_up(target, 12)
    find(target, "Renamed", scroll=True, contains=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Renamed", scroll=True, contains=True)
    scroll_up(target, 12)
    tap(target, "Weight (g)", scroll=True)
    adb(target, "shell", "input", "text", "4200")
    dismiss_keyboard(target)
    tap(target, "Length (mm)", scroll=True)
    adb(target, "shell", "input", "text", "540")
    dismiss_keyboard(target)
    tap(target, "Head circumference (mm)", scroll=True)
    adb(target, "shell", "input", "text", "350")
    dismiss_keyboard(target)
    tap(target, "Save growth", scroll=True)
    find(target, "Growth · 4200 g · 540 mm · head 350 mm", scroll=True)
    tap(target, "Edit growth", scroll=True)
    tap(target, "4200")
    adb(target, "shell", "input", "keyevent", "123")
    for _ in range(4):
        adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "text", "4300")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Growth · 4300 g · 540 mm · head 350 mm", scroll=True)
    tap(target, "Edit growth", scroll=True)
    tap(target, "350")
    adb(target, "shell", "input", "keyevent", "123")
    for _ in range(3):
        adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "text", "355")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Growth · 4300 g · 540 mm · head 355 mm", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Growth · 4300 g · 540 mm · head 355 mm", scroll=True)
    scroll_up(target, 12)
    tap(target, "Minutes slept", scroll=True)
    adb(target, "shell", "input", "text", "30")
    dismiss_keyboard(target)
    tap(target, "Crib", scroll=True)
    tap(target, "Save sleep", scroll=True)
    find(target, "Sleep · 30 min", scroll=True)
    find(target, "Place: Crib", scroll=True)
    tap(target, "Edit sleep duration", scroll=True)
    tap(target, "30")
    adb(target, "shell", "input", "keyevent", "123")
    for _ in range(2):
        adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "text", "20")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Sleep · 20 min", scroll=True)
    find(target, "Place: Crib", scroll=True)
    tap(target, "Edit sleep place", scroll=True)
    tap(target, "Pram")
    tap(target, "Save changes")
    find(target, "Place: Pram", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Sleep · 20 min", scroll=True)
    find(target, "Place: Pram", scroll=True)
    scroll_up(target, 12)
    tap(target, "Temperature (°C)", scroll=True)
    adb(target, "shell", "input", "text", "37.5")
    dismiss_keyboard(target)
    tap(target, "Save temperature", scroll=True)
    find(target, "Temperature · 37.5 °C", scroll=True)
    tap(target, "Edit temperature", scroll=True)
    tap(target, "37.5")
    adb(target, "shell", "input", "keyevent", "123")
    for _ in range(4):
        adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "text", "37.8")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    scroll_up(target, 10)
    find(target, "Temperature · 37.8 °C", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Temperature · 37.8 °C", scroll=True)
    scroll_up(target, 20)
    tap(target, "New Family", scroll=True)
    tap(target, "Child’s name", scroll=True)
    adb(target, "shell", "input", "text", "FirstChild")
    dismiss_keyboard(target)
    tap(target, "Add child", scroll=True)
    tap(target, "Child’s name", scroll=True)
    adb(target, "shell", "input", "text", "SelectedChild")
    dismiss_keyboard(target)
    tap(target, "Add child", scroll=True)
    tap(target, "What happened?", scroll=True)
    adb(target, "shell", "input", "text", "SelectedChildMarker")
    dismiss_keyboard(target)
    tap(target, "Save note", scroll=True)
    find(target, "Note · SelectedChildMarker", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    tap(target, "View timeline", scroll=True)
    find(target, "Note · SelectedChildMarker", scroll=True)
    print("Android UI Family/child selection, timeline, logging, edits, and restart: OK")


if __name__ == "__main__":
    main()
