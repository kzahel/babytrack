#!/usr/bin/env python3
"""Exercise one local caregiver flow through the installed Android UI."""

from __future__ import annotations

import argparse
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
    adb(target, "shell", "uiautomator", "dump", "/sdcard/babytrack-ui.xml")
    return ET.fromstring(adb(target, "exec-out", "cat", "/sdcard/babytrack-ui.xml"))


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


def main(quick: bool = False) -> None:
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
    tap(target, "Family options")
    find(target, "Sharing controls")
    tap(target, "Hide Family options")
    tap(target, "Add child", scroll=True)
    tap(target, "Child’s name", scroll=True)
    adb(target, "shell", "input", "text", "UITestChild")
    dismiss_keyboard(target)
    tap(target, "Add child", scroll=True)
    find(target, "Family 1 · UITestChild")
    tap(target, "Family", actionable=True)
    tap(target, "Child options", scroll=True)
    find(target, "Edit child profile", scroll=True)
    find(target, "Add another child", scroll=True)
    tap(target, "Hide child options", scroll=True)
    tap(target, "Today", actionable=True)
    tap(target, "View timeline", scroll=True)
    find(target, "No entries yet.")
    tap(target, "Today", actionable=True)
    tap(target, "Bottle", scroll=True)
    find(target, "Bottle amount")
    adb(target, "shell", "input", "keyevent", "4")
    open_capture(target, "Log completed sleep")
    find(target, "Minutes slept", scroll=True)
    adb(target, "shell", "input", "keyevent", "4")
    tap(target, "Wet now", scroll=True)
    tap(target, "History", actionable=True)
    find(target, "Diaper · Wet", scroll=True)
    tap(target, "Details and edits", scroll=True)
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
    find(target, "Family 1 · UITestChild")
    tap(target, "History", actionable=True)
    find(target, "Diaper · Dirty")
    tap(target, "Details and edits", scroll=True)
    tap(target, "Delete entry", scroll=True)
    find(target, "Remove this entry from the Family timeline? Shared devices receive the change when they sync.")
    tap(target, "Delete")
    find(target, "No entries yet.", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Family 1 · UITestChild")
    tap(target, "History", actionable=True)
    find(target, "No entries yet.", scroll=True)
    if quick:
        print("Android UI quick Family, diaper, edit, delete, and restart: OK")
        return
    open_capture(target, "Add note")
    tap(target, "What happened?", scroll=True)
    adb(target, "shell", "input", "text", "Before")
    dismiss_keyboard(target)
    tap(target, "Save note", scroll=True)
    open_history(target)
    find(target, "Note · Before", scroll=True)
    open_entry_actions(target, "Note · Before")
    tap(target, "Edit note", scroll=True)
    tap(target, "Before")
    adb(target, "shell", "input", "text", "After")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Note · BeforeAfter", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    open_history(target)
    find(target, "Note · BeforeAfter", scroll=True)
    open_capture(target, "Log diaper")
    tap(target, "Dry", scroll=True)
    open_history(target)
    find(target, "Diaper · Dry", scroll=True)
    open_capture(target, "Log bottle")
    tap(target, "Breast milk", scroll=True)
    tap(target, "Bottle amount", scroll=True)
    adb(target, "shell", "input", "text", "90")
    dismiss_keyboard(target)
    tap(target, "Log bottle", scroll=True, actionable=True)
    open_history(target)
    find(target, "Bottle · 90 mL · Breast milk", scroll=True)
    open_entry_actions(target, "Bottle · 90 mL · Breast milk")
    tap(target, "Edit bottle", scroll=True)
    tap(target, "Formula")
    tap(target, "Bottle amount")
    adb(target, "shell", "input", "keyevent", "123")
    adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "text", "120")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Bottle · 120 mL · Formula", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    open_capture(target, "Log breast feed")
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
    open_history(target)
    find(target, "Breast · Left 5 min → Right 8 min", scroll=True)
    open_entry_actions(target, "Breast · Left 5 min → Right 8 min")
    tap(target, "Edit breast feed", scroll=True)
    tap(target, "Right")
    tap(target, "Save changes")
    find(target, "Breast · Right 5 min → Right 8 min", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    open_history(target)
    find(target, "Breast · Right 5 min → Right 8 min", scroll=True)
    scroll_up(target)
    find(target, "Bottle · 120 mL · Formula", scroll=True)
    open_entry_actions(target, "Bottle · 120 mL · Formula")
    tap(target, "Edit bottle", scroll=True)
    tap(target, "US fl oz")
    tap(target, "Bottle amount")
    adb(target, "shell", "input", "text", "4.5")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Bottle · 4.5 US fl oz · Formula", scroll=True)
    dismiss_keyboard(target)
    tap(target, "Edit bottle", scroll=True, actionable=True)
    tap(target, "UK fl oz")
    tap(target, "Bottle amount")
    adb(target, "shell", "input", "text", "4.5")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Bottle · 4.5 UK fl oz · Formula", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    open_history(target)
    find(target, "Bottle · 4.5 UK fl oz · Formula", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    open_family(target)
    tap(target, "Child options", scroll=True)
    tap(target, "Edit child profile", scroll=True)
    tap(target, "Child’s name", scroll=True)
    adb(target, "shell", "input", "keyevent", "123")
    adb(target, "shell", "input", "keyevent", *(["67"] * 30))
    adb(target, "shell", "input", "text", "Renamed")
    dismiss_keyboard(target)
    find(target, "Renamed", contains=True)
    tap(target, "Save changes", scroll=True)
    scroll_up(target, 12)
    find(target, "Renamed", scroll=True, contains=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    find(target, "Renamed", scroll=True, contains=True)
    open_capture(target, "Log growth")
    tap(target, "Weight", scroll=True)
    adb(target, "shell", "input", "text", "4.2")
    dismiss_keyboard(target)
    tap(target, "Length", scroll=True)
    adb(target, "shell", "input", "text", "54")
    dismiss_keyboard(target)
    tap(target, "Head circumference", scroll=True)
    adb(target, "shell", "input", "text", "35")
    dismiss_keyboard(target)
    tap(target, "Save growth", scroll=True)
    open_history(target)
    find(target, "Growth · 4.2 kg · 54 cm · head 35 cm", scroll=True)
    open_entry_actions(target, "Growth · 4.2 kg · 54 cm · head 35 cm")
    tap(target, "Edit growth", scroll=True)
    tap(target, "4.2", scroll=True)
    adb(target, "shell", "input", "keyevent", "123")
    for _ in range(3):
        adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "text", "4.3")
    dismiss_keyboard(target)
    tap(target, "Save changes", scroll=True)
    find(target, "Growth · 4.3 kg · 54 cm · head 35 cm", scroll=True)
    tap(target, "Edit growth", scroll=True)
    scroll_dialog_down(target)
    tap(target, "35")
    adb(target, "shell", "input", "keyevent", "123")
    for _ in range(2):
        adb(target, "shell", "input", "keyevent", "67")
    adb(target, "shell", "input", "text", "35.5")
    dismiss_keyboard(target)
    tap(target, "Save changes", scroll=True)
    find(target, "Growth · 4.3 kg · 54 cm · head 35.5 cm", scroll=True)
    tap(target, "Add note to entry", scroll=True)
    tap(target, "What happened?")
    adb(target, "shell", "input", "text", "GrowthNote")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Note: GrowthNote", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    open_history(target)
    find(target, "Growth · 4.3 kg · 54 cm · head 35.5 cm", scroll=True)
    find(target, "Note: GrowthNote", scroll=True)
    open_capture(target, "Log completed sleep")
    tap(target, "Minutes slept", scroll=True)
    adb(target, "shell", "input", "text", "30")
    dismiss_keyboard(target)
    tap(target, "Crib", scroll=True)
    tap(target, "Save sleep", scroll=True)
    open_history(target)
    find(target, "Sleep · 30 min", scroll=True)
    find(target, "Place: Crib", scroll=True)
    open_entry_actions(target, "Sleep · 30 min")
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
    open_history(target)
    find(target, "Sleep · 20 min", scroll=True)
    find(target, "Place: Pram", scroll=True)
    open_capture(target, "Log temperature")
    tap(target, "Temperature (°C)", scroll=True)
    adb(target, "shell", "input", "text", "37.5")
    dismiss_keyboard(target)
    tap(target, "Save temperature", scroll=True)
    open_history(target)
    find(target, "Temperature · 37.5 °C", scroll=True)
    open_entry_actions(target, "Temperature · 37.5 °C")
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
    open_history(target)
    find(target, "Temperature · 37.8 °C", scroll=True)
    open_entry_actions(target, "Temperature · 37.8 °C")
    dismiss_keyboard(target)
    tap(target, "Edit temperature", scroll=True, actionable=True)
    tap(target, "°F")
    tap(target, "Temperature (°F)")
    adb(target, "shell", "input", "text", "99")
    dismiss_keyboard(target)
    tap(target, "Save changes")
    find(target, "Temperature · 99 °F", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    open_history(target)
    find(target, "Temperature · 99 °F", scroll=True)
    open_family(target)
    tap(target, "Family options", scroll=True)
    tap(target, "New Family", scroll=True)
    tap(target, "Add child", scroll=True)
    tap(target, "Child’s name", scroll=True)
    adb(target, "shell", "input", "text", "FirstChild")
    dismiss_keyboard(target)
    tap(target, "Add child", scroll=True)
    open_family(target)
    tap(target, "Child options", scroll=True)
    tap(target, "Add another child", scroll=True)
    tap(target, "Child’s name", scroll=True)
    adb(target, "shell", "input", "text", "SelectedChild")
    dismiss_keyboard(target)
    tap(target, "Add child", scroll=True)
    open_capture(target, "Add note")
    tap(target, "What happened?", scroll=True)
    adb(target, "shell", "input", "text", "SelectedChildMarker")
    dismiss_keyboard(target)
    tap(target, "Save note", scroll=True)
    open_history(target)
    find(target, "Note · SelectedChildMarker", scroll=True)
    adb(target, "shell", "am", "force-stop", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    open_history(target)
    find(target, "Note · SelectedChildMarker", scroll=True)
    print("Android UI Family/child selection, timeline, logging, edits, and restart: OK")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--quick", action="store_true", help="Run the short push/PR UI path")
    main(parser.parse_args().quick)
