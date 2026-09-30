#!/usr/bin/env python3
"""Build an offline screen catalog from the Android renderer's manifest files."""

import argparse
import html
import json
import shutil
import struct
import subprocess
from pathlib import Path

VARIANTS = ("dark", "light", "dark-large", "light-large")


def build_gallery(output: Path) -> None:
    cases = {}
    for path in sorted(output.glob("*--*.json")):
        record = json.loads(path.read_text())
        case_id, variant = record["id"], record["variant"]
        if variant not in VARIANTS or variant in cases.setdefault(case_id, {}):
            raise ValueError(f"Duplicate or unknown variant in {path}")
        if not record["pages"]:
            raise ValueError(f"Missing rendered pages in {path}")
        for page in record["pages"]:
            image = output / page["file"]
            if image.parent != output or image.suffix != ".png":
                raise ValueError(f"Invalid image path: {image}")
            header = image.read_bytes()[:24]
            if header[:8] != b"\x89PNG\r\n\x1a\n":
                raise ValueError(f"Invalid PNG: {image}")
            page["widthPx"], page["heightPx"] = struct.unpack(">II", header[16:24])
            if page["widthPx"] < 300 or page["heightPx"] < 600:
                raise ValueError(f"Unexpectedly small screen: {image}")
        cases[case_id][variant] = record
    if not cases:
        raise ValueError(f"No screen renders in {output}")
    for case_id, variants in cases.items():
        if set(variants) != set(VARIANTS):
            raise ValueError(f"Incomplete variant matrix for {case_id}")
    viewports = {(record.get("viewport", "compact"), record["widthDp"], record["heightDp"], record["density"])
                 for variants in cases.values() for record in variants.values()}
    if len(viewports) != 1:
        raise ValueError("Mixed viewport metadata; render each viewport into its own gallery")
    viewport, width_dp, height_dp, density = next(iter(viewports))
    display = f'{viewport.title()} viewport · {width_dp} × {height_dp} dp · {density:g}× image scale'
    # Stable metadata and IDs can become visual baselines later. No pixel gate yet.
    repository = Path(__file__).resolve().parent.parent
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
    dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=repository, text=True).strip())
    manifest = {"schemaVersion": 1, "sourceRevision": revision, "workingTreeDirty": dirty, "renderer": {"roborazzi": "1.39.0", "robolectric": "4.16.1"},
                "cases": [record for variants in cases.values() for record in variants.values()]}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    groups = [{"id": key, "label": label} for key, label in [
        ("today", "Today"), ("history", "History"), ("family", "Family"),
        ("capture", "Capture"), ("child", "Child profile"), ("other", "Other"),
    ]]
    states = [{"id": key, "label": label} for key, label in [
        ("default", "Default"), ("empty", "Empty / first run"),
        ("pending", "Pending / joining"), ("attention", "Needs attention"),
        ("running", "Running timer"), ("editing", "Editing / options"),
        ("edge", "Layout edge cases"),
    ]]
    overrides = {
        "today-empty": "empty", "history-empty": "empty", "family-first-run": "empty",
        "capture-bottle-empty": "empty", "today-sync-pending": "pending",
        "family-join-pending": "pending", "family-shared-pending": "pending",
        "today-blocked": "attention", "family-removed": "attention",
        "capture-bottle-invalid": "attention", "today-timer": "running",
        "history-entry-actions": "editing", "history-filtered": "editing",
        "family-options": "editing", "child-create": "editing", "child-edit": "editing",
        "capture-note-long-name": "edge",
    }
    order = {group["id"]: index for index, group in enumerate(groups)}
    state_order = {state["id"]: index for index, state in enumerate(states)}
    group_labels = {group["id"]: group["label"] for group in groups}
    rows = []
    for case_id, variants in cases.items():
        label = variants["dark"]["label"]
        group = case_id.split("-")[0]
        group = group if group in order else "other"
        rows.append({"id": case_id, "label": label,
                     "title": label.removeprefix(group_labels[group] + " · "), "group": group,
                     "state": overrides.get(case_id, "default"), "variants": variants})
    rows.sort(key=lambda row: (order[row["group"]], state_order[row["state"]], row["id"]))
    used_groups = {row["group"] for row in rows}
    data = json.dumps({"cases": rows, "groups": [g for g in groups if g["id"] in used_groups],
                       "states": states}, ensure_ascii=False).replace("<", "\\u003c")
    assets = Path(__file__).with_name("android-gallery")
    document = (assets / "index.html").read_text()
    (output / "index.html").write_text(document.replace("__COUNT__", str(len(cases)))
        .replace("__DISPLAY__", html.escape(display)).replace("__DATA__", data))
    for name in ("gallery.css", "gallery.js"):
        shutil.copyfile(assets / name, output / name)
    print(f"Gallery: {output / 'index.html'} ({len(cases)} cases, {len(cases) * 4} variants)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    build_gallery(parser.parse_args().output.resolve())
