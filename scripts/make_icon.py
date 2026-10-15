#!/usr/bin/env python3
"""Draws Pulse's icon (1024×1024 PNG): a dark rounded square with an orange pulse
line. Standard library only. Usage: make_icon.py out.png"""
import math, struct, sys, zlib

SIZE = 1024
MARGIN = 100            # macOS icons leave room around the square
RADIUS = 185
HALF_WIDTH = 26         # of the pulse line
ORANGE = (0xFF, 0xA8, 0x24)

# The pulse: flat, a small bump, a tall spike up and down, a bump, flat.
POINTS = [(190, 560), (360, 560), (405, 505), (450, 560), (505, 560), (565, 300),
          (630, 760), (680, 560), (735, 560), (775, 520), (815, 560), (834, 560)]


def rounded_square_distance(x, y):
    """Signed distance to the rounded square (negative inside)."""
    c = SIZE / 2
    half = c - MARGIN
    qx, qy = abs(x - c) - (half - RADIUS), abs(y - c) - (half - RADIUS)
    outside = math.hypot(max(qx, 0), max(qy, 0))
    return outside + min(max(qx, qy), 0) - RADIUS


def segment_distance(px, py, a, b):
    (ax, ay), (bx, by) = a, b
    dx, dy = bx - ax, by - ay
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def coverage(distance):
    return max(0.0, min(1.0, 0.5 - distance))


def main(out):
    segments = list(zip(POINTS, POINTS[1:]))
    rows = []
    for y in range(SIZE):
        row = bytearray([0])
        for x in range(SIZE):
            px, py = x + 0.5, y + 0.5
            square = coverage(rounded_square_distance(px, py))
            if square == 0:
                row += b"\0\0\0\0"
                continue
            # Background: a soft vertical gradient.
            shade = 44 - 22 * (py - MARGIN) / (SIZE - 2 * MARGIN)
            r = g = b = shade
            line = 0.0
            if 200 < py < 860:
                d = min(segment_distance(px, py, a, b2) for a, b2 in segments)
                line = coverage(d - HALF_WIDTH)
                # A faint glow under the line.
                glow = max(0.0, 1 - (d - HALF_WIDTH) / 60) * 0.18 if d > HALF_WIDTH else 0
                r, g, b = (r + (ORANGE[0] - r) * glow, g + (ORANGE[1] - g) * glow, b + (ORANGE[2] - b) * glow)
            r, g, b = (r + (ORANGE[0] - r) * line, g + (ORANGE[1] - g) * line, b + (ORANGE[2] - b) * line)
            row += bytes([round(r), round(g), round(b), round(255 * square)])
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    with open(out, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    main(sys.argv[1])
