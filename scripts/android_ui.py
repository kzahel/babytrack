"""ADB/UIAutomator helpers for command-driven disposable-emulator checks.

Importing this module never installs an APK or runs a caregiver scenario.
"""

from __future__ import annotations

import os
import re
import subprocess
import time
from datetime import datetime, timezone
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APK = ROOT / "apps/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE = "org.babytrack.app"


def command(*args: str, timeout: float = 60) -> str:
    for attempt in range(4):
        result = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
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


def adb(target: str, *args: str, timeout: float = 60) -> str:
    return command("adb", "-s", target, *args, timeout=timeout)


def stage(label: str) -> None:
    print(f"[{datetime.now(timezone.utc).isoformat(timespec='seconds')}] {label}", flush=True)


def capture_failure(target: str, name: str) -> Path:
    output = Path(os.environ.get("BABYTRACK_ANDROID_DIAGNOSTICS_DIR", str(
        ROOT / "apps/android/app/build/outputs/ui-diagnostics"))) / f"{name}-{time.time_ns()}"
    output.mkdir(parents=True, exist_ok=True)
    commands = {
        "screen.png": ("exec-out", "screencap", "-p"),
        "logcat.txt": ("logcat", "-d", "-t", "500"),
        "last-ui.xml": ("exec-out", "cat", "/sdcard/babytrack-ui.xml"),
    }
    for filename, args in commands.items():
        try:
            result = subprocess.run(("adb", "-s", target, *args), capture_output=True, timeout=10)
            if result.returncode == 0:
                (output / filename).write_bytes(result.stdout)
            else:
                (output / f"{filename}.error.txt").write_bytes(result.stderr)
        except Exception as cause:
            (output / f"{filename}.error.txt").write_text(str(cause))
    return output


def run_scenario(name: str, action) -> None:
    target = serial()
    if adb(target, "shell", "getprop", "ro.kernel.qemu").strip() != "1":
        raise RuntimeError("This scenario runs only on a disposable emulator")
    stage(f"{name}: start on {target}")
    try:
        action()
    except BaseException:
        try:
            stage(f"{name}: failure artifacts at {capture_failure(target, name)}")
        except Exception as cause:
            stage(f"{name}: could not save diagnostics: {cause}")
        raise
    stage(f"{name}: complete")


def nodes(target: str) -> ET.Element:
    # uiautomator can return exit zero before the first frame is idle, without
    # creating a dump. Never parse a stale file or cat's error as screen XML.
    last_dump = ""
    for attempt in range(3):
        adb(target, "shell", "rm", "-f", "/sdcard/babytrack-ui.xml")
        last_dump = adb(target, "shell", "uiautomator", "dump", "/sdcard/babytrack-ui.xml", timeout=20)
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
            # Focus the gesture on the scrollable content, not a pinned footer.
            areas = [node for node in root.iter("node")
                     if node.attrib.get("scrollable") == "true"]
            bounds = [list(map(int, re.findall(r"\d+", node.attrib["bounds"])))
                      for node in areas]
            if bounds:
                x1, y1, x2, y2 = max(bounds, key=lambda box: (box[2] - box[0]) * (box[3] - box[1]))
                x = (x1 + x2) // 2
                low, high = y1 + (y2 - y1) * 4 // 5, y1 + (y2 - y1) // 4
            else:
                x, low, high = width // 2, height * 7 // 10, height * 4 // 10
            adb(target, "shell", "input", "swipe", str(x),
                str(low if down else high), str(x),
                str(high if down else low), "360")
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


def dismiss_keyboard(target: str) -> None:
    if "mInputShown=true" in adb(target, "shell", "dumpsys", "input_method"):
        adb(target, "shell", "input", "keyevent", "4")


def tap_tab(target: str, label: str) -> None:
    # After a cold start the tracker may still be loading; wait for the bar.
    deadline = time.monotonic() + 20
    while True:
        root = nodes(target)
        matches = [node for node in root.iter("node") if node.attrib.get("text") == label]
        if matches or time.monotonic() > deadline:
            break
        time.sleep(0.4)
    if not matches:
        raise AssertionError(f"Navigation destination {label!r} is absent")
    x1, y1, x2, y2 = map(int, re.findall(r"\d+", matches[-1].attrib["bounds"]))
    adb(target, "shell", "input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))


def open_capture(target: str, activity: str) -> None:
    stage(f"Capture: {activity}")
    tap_tab(target, "Today")
    tap(target, "Add activity", scroll=True, actionable=True)
    tap(target, activity, scroll=True, actionable=True)


def open_history(target: str) -> None:
    tap_tab(target, "History")


def open_family(target: str) -> None:
    tap_tab(target, "Family")


def open_entry_actions(target: str, entry_label: str) -> None:
    """Expand the actions on the named History row, even after scrolling.

    Revealed actions sit directly below their row, indented past its icon.
    The row is tapped only when the space below it is visible, so an
    already expanded row is never collapsed by mistake.
    """
    stage(f"History correction: {entry_label}")
    find(target, entry_label, scroll=True)
    sizes = re.findall(r"(\d+)x(\d+)", adb(target, "shell", "wm", "size"))
    width, height = map(int, sizes[-1])

    def bounds(node: ET.Element) -> list[int]:
        return list(map(int, re.findall(r"\d+", node.attrib["bounds"])))

    def locate() -> tuple[list[int], list[int], ET.Element] | None:
        root = nodes(target)
        parents = {child: parent for parent in root.iter() for child in parent}
        row = next((n for n in root.iter("node") if n.attrib.get("text") == entry_label), None)
        while row is not None and row.attrib.get("clickable") != "true" and row in parents:
            row = parents[row]
        if row is None or row.attrib.get("clickable") != "true":
            return None
        # The main column is the tallest scrollable node; a short sideways chip
        # row is not. Content that fits reports no vertical scroller, so fall
        # back to the region above the navigation bar.
        areas = [bounds(n) for n in root.iter("node") if n.attrib.get("scrollable") == "true"]
        tall = [b for b in areas if b[3] - b[1] > height // 3]
        tabs = [bounds(n)[1] for n in root.iter("node")
                if n.attrib.get("text") in ("Today", "History", "Family")]
        bottom = min([y for y in tabs if y > height // 2], default=height)
        area = max(tall, key=lambda b: b[3] - b[1]) if tall else [0, 0, width, bottom]
        return bounds(row), area, root

    def expanded(row: list[int], area: list[int], root: ET.Element) -> bool:
        x1, _, _, y2 = row
        for node in root.iter("node"):
            if node.attrib.get("clickable") == "true":
                bx1, by1, _, _ = bounds(node)
                # Only content inside the scroll area counts, not the navigation bar.
                if y2 - 4 <= by1 <= y2 + 40 and by1 < area[3] - 8 and bx1 > x1 + 30:
                    return True
        return False

    for _ in range(8):
        found = locate()
        if found is None:
            raise AssertionError(f"History row {entry_label!r} is not visible")
        row, area, root = found
        if expanded(row, area, root):
            return
        if row[3] + 48 > area[3]:
            # Too close to the bottom to see what is below; scroll if possible.
            adb(target, "shell", "input", "swipe", str(width // 2), str(area[3] - 20),
                str(width // 2), str(max(area[1] + 20, area[3] - 160)), "300")
            time.sleep(0.4)
            moved = locate()
            if moved is not None and moved[0][3] != row[3]:
                continue
        x1, y1, x2, y2 = row
        adb(target, "shell", "input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))
        time.sleep(0.5)
    raise AssertionError(f"Could not open actions for {entry_label!r}")
