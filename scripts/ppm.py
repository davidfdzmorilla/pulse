#!/usr/bin/env python3
"""Converts a PPM screenshot to PNG, optionally a crop of it (standard library only).
Usage: ppm.py in.ppm out.png [x y width height]"""
import struct, sys, zlib

data = open(sys.argv[1], "rb").read()
header, _, pixels = data.partition(b"\n")
_, width, height, _ = header.split()
width, height = int(width), int(height)
x, y, w, h = map(int, sys.argv[3:7]) if len(sys.argv) >= 7 else (0, 0, width, height)
w, h = min(w, width - x), min(h, height - y)
rows = b"".join(b"\0" + pixels[((y + r) * width + x) * 3:((y + r) * width + x + w) * 3] for r in range(h))


def chunk(kind, body):
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)


png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(rows, 6)) + chunk(b"IEND", b"")
open(sys.argv[2], "wb").write(png)
