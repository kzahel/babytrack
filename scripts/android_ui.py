"""ADB/UIAutomator helpers for command-driven disposable-emulator checks.

Importing this module never installs an APK or runs a caregiver scenario.
"""

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
    for attempt in range(4):
        result = subprocess.run(args, capture_output=True, text=True)
        if result.returncode == 0:
            return result.stdout
        if args[0] != "adb" or result.returncode != 255 or attempt == 3:
            result.check_returncode()
        time.sleep(1)
    raise AssertionError("unreachable")


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
    # uiautomator can return exit zero before the first frame is idle, without
    # creating a dump. Never parse a stale file or cat's error as screen XML.
    last_dump = ""
    for attempt in range(3):
        adb(target, "shell", "rm", "-f", "/sdcard/babytrack-ui.xml")
        last_dump = adb(target, "shell", "uiautomator", "dump", "/sdcard/babytrack-ui.xml")
        if "dumped to:" in last_dump:
            return ET.fromstring(adb(target, "exec-out", "cat", "/sdcard/babytrack-ui.xml"))
        if attempt < 2:
            time.sleep(0.3)
    raise RuntimeError(f"Android UI dump was not created: {last_dump.strip()}")


def find(target: str, label: str, *, scroll: bool = False, occurrence: int = 0,
         contains: bool = False, actionable: bool = False) -> ET.Element:
    deadline = time.monotonic() + (100 if scroll else 20)
    scroll_count = 0
    visible: list[str] = []
    sizes = re.findall(r"(\d+)x(\d+)", adb(target, "shell", "wm", "size"))
    if not sizes:
        raise RuntimeError("Android display size unavailable")
    width, height = map(int, sizes[-1])
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
                str(height * 7 // 10 if down else height * 4 // 10), str(width // 2),
                str(height * 4 // 10 if down else height * 7 // 10), "360",
            )
            time.sleep(0.25)
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
    sizes = re.findall(r"(\d+)x(\d+)", adb(target, "shell", "wm", "size"))
    if not sizes:
        raise RuntimeError("Android display size unavailable")
    width, height = map(int, sizes[-1])
    for _ in range(times):
        adb(target, "shell", "input", "swipe", str(width // 2),
            str(height * 3 // 10), str(width // 2), str(height * 4 // 5), "300")


def scroll_dialog_down(target: str, times: int = 2) -> None:
    sizes = re.findall(r"(\d+)x(\d+)", adb(target, "shell", "wm", "size"))
    if not sizes:
        raise RuntimeError("Android display size unavailable")
    width, height = map(int, sizes[-1])
    for _ in range(times):
        adb(target, "shell", "input", "swipe", str(width // 2),
            str(height * 62 // 100), str(width // 2), str(height * 38 // 100), "350")


def dismiss_keyboard(target: str) -> None:
    if "mInputShown=true" in adb(target, "shell", "dumpsys", "input_method"):
        adb(target, "shell", "input", "keyevent", "4")


def tap_tab(target: str, label: str) -> None:
    root = nodes(target)
    matches = [node for node in root.iter("node") if node.attrib.get("text") == label]
    if not matches:
        raise AssertionError(f"Navigation destination {label!r} is absent")
    x1, y1, x2, y2 = map(int, re.findall(r"\d+", matches[-1].attrib["bounds"]))
    adb(target, "shell", "input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))


def open_capture(target: str, activity: str) -> None:
    tap_tab(target, "Today")
    tap(target, "Add activity", scroll=True, actionable=True)
    tap(target, activity, scroll=True, actionable=True)


def open_history(target: str) -> None:
    tap_tab(target, "History")


def open_family(target: str) -> None:
    tap_tab(target, "Family")


def open_entry_actions(target: str, entry_label: str) -> None:
    """Expand the actions on the named History row, even after scrolling."""
    find(target, entry_label, scroll=True)
    for _ in range(4):
        root = nodes(target)
        parents = {child: parent for parent in root.iter() for child in parent}
        labels = [node for node in root.iter("node") if node.attrib.get("text") == entry_label]
        if labels:
            ancestor = labels[0]
            while ancestor in parents:
                ancestor = parents[ancestor]
                buttons = [node for node in ancestor.iter("node")
                           if node.attrib.get("text") in ("Details and edits", "Hide details")]
                if len(buttons) == 1:
                    if buttons[0].attrib["text"] == "Details and edits":
                        x1, y1, x2, y2 = map(int, re.findall(r"\d+", buttons[0].attrib["bounds"]))
                        adb(target, "shell", "input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))
                    return
        # The label can be at the bottom of the viewport while its actions
        # are just below it. Bring the rest of the row into view.
        sizes = re.findall(r"(\d+)x(\d+)", adb(target, "shell", "wm", "size"))
        width, height = map(int, sizes[-1])
        adb(target, "shell", "input", "swipe", str(width // 2), str(height * 7 // 10),
            str(width // 2), str(height * 5 // 10), "300")
    raise AssertionError(f"No details action found for {entry_label!r}")


