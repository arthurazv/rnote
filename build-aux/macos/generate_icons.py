#!/usr/bin/env python3
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
OUTPUT_DIR = Path(__file__).resolve().parent

def make_icns(svg_path: Path, output_icns: Path):
    iconset = output_icns.with_suffix(".iconset")
    if iconset.exists():
        shutil.rmtree(iconset)
    iconset.mkdir(parents=True, exist_ok=True)

    sizes = [16, 32, 64, 128, 256, 512, 1024]
    for s in sizes:
        if s <= 512:
            subprocess.run(
                ["rsvg-convert", "-w", str(s), "-h", str(s), str(svg_path), "-o", str(iconset / f"icon_{s}x{s}.png")],
                check=True,
            )
        if s >= 32:
            half = s // 2
            subprocess.run(
                ["rsvg-convert", "-w", str(s), "-h", str(s), str(svg_path), "-o", str(iconset / f"icon_{half}x{half}@2x.png")],
                check=True,
            )

    subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(output_icns)], check=True)
    shutil.rmtree(iconset)
    print(f"Generated {output_icns} ({output_icns.stat().st_size} bytes)")

def main():
    app_icon = ROOT / "crates/rnote-ui/data/icons/scalable/apps/rnote.svg"
    doc_icon = ROOT / "crates/rnote-ui/data/icons/scalable/mimetypes/application-rnote.svg"

    make_icns(app_icon, OUTPUT_DIR / "Rnote.icns")
    make_icns(doc_icon, OUTPUT_DIR / "RnoteDoc.icns")

if __name__ == "__main__":
    main()
