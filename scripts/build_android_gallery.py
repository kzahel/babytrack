#!/usr/bin/env python3
"""Build an offline scrolling gallery from the Android renderer's manifest files."""

import argparse
import html
import json
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
    # Stable metadata and IDs can become visual baselines later. No pixel gate yet.
    repository = Path(__file__).resolve().parent.parent
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
    dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=repository, text=True).strip())
    manifest = {"schemaVersion": 1, "sourceRevision": revision, "workingTreeDirty": dirty, "renderer": {"roborazzi": "1.39.0", "robolectric": "4.16.1"},
                "cases": [record for variants in cases.values() for record in variants.values()]}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    sections = []
    order = {"today": 0, "history": 1, "family": 2, "capture": 3, "child": 4}
    for case_id in sorted(cases, key=lambda key: (order.get(key.split("-")[0], 9), key)):
        variants = cases[case_id]
        label = html.escape(variants["dark"]["label"])
        cards = []
        for variant in VARIANTS:
            record = variants[variant]
            frames = []
            for index, page in enumerate(record["pages"]):
                filename = html.escape(page["file"], quote=True)
                frames.append(f'<figure><a href="{filename}"><img src="{filename}" loading="lazy" '
                              f'width="{page["widthPx"]}" height="{page["heightPx"]}" '
                              f'alt="{label}, {variant}, page {index + 1}"></a>'
                              f'<figcaption>Page {index + 1} · scroll {page["scrollDp"]} dp</figcaption></figure>')
            more = f'<details><summary>{len(frames) - 1} more scroll positions</summary>{"".join(frames[1:])}</details>' if len(frames) > 1 else ""
            cards.append(f'<article data-variant="{variant}"><h3>{variant.replace("-large", " · 1.5× text")}</h3>{frames[0]}{more}</article>')
        sections.append(f'<section id="{case_id}" data-search="{html.escape(case_id + " " + variants["dark"]["label"], quote=True)}">'
                        f'<h2><a href="#{case_id}">{label}</a></h2><div class="screens">{"".join(cards)}</div></section>')
    document = '''<!doctype html>
<html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Babytrack Android screen gallery</title>
<style>
:root{color-scheme:dark;font-family:system-ui,sans-serif;background:#111820;color:#f1f5f6}
body{margin:0;padding:24px;max-width:1600px;margin-inline:auto}header{position:sticky;top:0;background:#111820f5;padding:12px 0;z-index:1}
h1{margin:0;font-size:24px}p{color:#b8c5cd}input,button{font:inherit;padding:10px;border:1px solid #71818d;border-radius:8px;background:#1c2934;color:inherit}
input{min-width:240px}.controls{display:flex;gap:12px;flex-wrap:wrap}a{color:inherit}h2{margin-top:36px;font-size:20px;scroll-margin-top:170px}
.screens{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:18px}article{min-width:0}h3{font-size:15px;color:#c4d9df;font-weight:500}
figure{margin:0 0 14px}img{width:100%;height:auto;display:block;border-radius:12px;border:1px solid #465966}figcaption{font-size:12px;color:#afbcc8;margin-top:6px}
summary{cursor:pointer;padding:8px 0}details[open] summary{margin-bottom:12px}[hidden]{display:none!important}
@media(max-width:1000px){.screens{grid-template-columns:repeat(2,minmax(0,1fr))}}@media(max-width:550px){.screens{grid-template-columns:1fr}body{padding:12px}}
</style>
<header><h1>Babytrack · Android screen gallery</h1>
<p>__COUNT__ synthetic cases · actual Compose screens · 360 × 800 dp · API 35 · en-US · UTC · fixed September 29, 2026<br>
Dark and light, normal and 1.5× text. Additional scroll positions reveal long forms. Click an image for full size.</p>
<div class="controls"><input id="search" type="search" placeholder="Find a screen or state" aria-label="Find a screen or state">
<button id="expand">Expand all scroll positions</button><a href="manifest.json">Rendering manifest</a></div></header>
<main>__SECTIONS__</main>
<script>
document.querySelector('#search').addEventListener('input',e=>{const q=e.target.value.toLowerCase();document.querySelectorAll('section').forEach(s=>s.hidden=!s.dataset.search.toLowerCase().includes(q))});
document.querySelector('#expand').addEventListener('click',e=>{const open=e.target.textContent.startsWith('Expand');document.querySelectorAll('details').forEach(d=>d.open=open);e.target.textContent=open?'Collapse scroll positions':'Expand all scroll positions'});
</script></html>
'''
    (output / "index.html").write_text(document.replace("__COUNT__", str(len(cases))).replace("__SECTIONS__", "\n".join(sections)))
    print(f"Gallery: {output / 'index.html'} ({len(cases)} cases, {len(cases) * 4} variants)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    build_gallery(parser.parse_args().output.resolve())
