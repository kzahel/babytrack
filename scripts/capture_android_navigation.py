#!/usr/bin/env python3
"""Capture Android navigation screens on a dedicated emulator for visual QA."""

from __future__ import annotations

import argparse
import subprocess
import time
from pathlib import Path

from android_ui import (
    APK, PACKAGE, ROOT, adb, dismiss_keyboard, find, serial, tap, tap_tab, run_scenario, stage,
)


OUTPUT = ROOT / "local-references" / "android-redesign-qa"


def capture(target: str, name: str) -> None:
    stage(f"Navigation capture: {name}")
    screenshot = subprocess.run(
        ["adb", "-s", target, "exec-out", "screencap", "-p"],
        check=True, capture_output=True, timeout=20,
    ).stdout
    (OUTPUT / f"{name}.png").write_bytes(screenshot)


def main() -> None:
    target = serial()
    if adb(target, "shell", "getprop", "ro.kernel.qemu").strip() != "1":
        raise RuntimeError("Visual QA capture runs only on an emulator")
    OUTPUT.mkdir(parents=True, exist_ok=True)
    adb(target, "install", "-r", str(APK))
    adb(target, "shell", "pm", "clear", PACKAGE)
    adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
    tap(target, "New Family")
    tap(target, "Add child", scroll=True)
    find(target, "Create child profile")
    capture(target, "create-child-profile")
    tap(target, "Child’s name", scroll=True)
    adb(target, "shell", "input", "text", "CaptureChild")
    dismiss_keyboard(target)
    tap(target, "Add child", scroll=True)
    find(target, "Family 1 · CaptureChild")
    original_scale = adb(target, "shell", "settings", "get", "system", "font_scale").strip()
    original_night = adb(target, "shell", "cmd", "uimode", "night").lower()
    try:
        for theme, scale in (("light", "1.0"), ("dark", "1.5")):
            adb(target, "shell", "settings", "put", "system", "font_scale", scale)
            adb(target, "shell", "cmd", "uimode", "night", "yes" if theme == "dark" else "no")
            adb(target, "shell", "am", "force-stop", PACKAGE)
            adb(target, "shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
            find(target, "Today")
            time.sleep(0.5)
            capture(target, f"{theme}-{scale}-today")
            tap_tab(target, "History")
            find(target, "Timeline")
            capture(target, f"{theme}-{scale}-history")
            tap_tab(target, "Family")
            find(target, "Family options")
            capture(target, f"{theme}-{scale}-family")
            tap(target, "Child options", scroll=True)
            tap(target, "Edit child profile", scroll=True)
            find(target, "Edit child profile")
            capture(target, f"{theme}-{scale}-edit-child-profile")
            adb(target, "shell", "input", "keyevent", "4")
            tap_tab(target, "Today")
            tap(target, "Add activity", scroll=True, actionable=True)
            find(target, "Log completed sleep", scroll=True)
            capture(target, f"{theme}-{scale}-capture")
    finally:
        adb(target, "shell", "settings", "put", "system", "font_scale", original_scale)
        mode = "auto" if "auto" in original_night else ("yes" if "yes" in original_night else "no")
        adb(target, "shell", "cmd", "uimode", "night", mode)
    print(f"Android navigation captures: {OUTPUT}")


if __name__ == "__main__":
    argparse.ArgumentParser(description=__doc__).parse_args()
    run_scenario("navigation-capture", main)
