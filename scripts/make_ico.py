#!/usr/bin/env python3
"""Packs Pulse's icon for Windows (packaging/windows/pulse.ico) from the 1024 px
PNG: PNG images of 16 to 256 px in one .ico (Windows Vista and later read them).
The sizes are made with macOS's sips. Standard library only.
Usage: scripts/make_ico.py [packaging/icon-1024.png] [packaging/windows/pulse.ico]"""
import os, struct, subprocess, sys, tempfile

SIZES = [256, 64, 48, 32, 24, 16]

source = sys.argv[1] if len(sys.argv) > 1 else "packaging/icon-1024.png"
target = sys.argv[2] if len(sys.argv) > 2 else "packaging/windows/pulse.ico"
images = []
with tempfile.TemporaryDirectory() as work:
    for size in SIZES:
        png = os.path.join(work, f"{size}.png")
        subprocess.run(["sips", "-z", str(size), str(size), source, "--out", png], check=True, capture_output=True)
        images.append((size, open(png, "rb").read()))
header = struct.pack("<HHH", 0, 1, len(images))
offset = len(header) + 16 * len(images)
entries, data = b"", b""
for size, png in images:
    # 256 is written as 0.
    entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(png), offset + len(data))
    data += png
open(target, "wb").write(header + entries + data)
print(f"{target}: {', '.join(str(s) for s in SIZES)} px")
