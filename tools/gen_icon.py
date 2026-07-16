#!/usr/bin/env python3
"""Generate a 1024x1024 traffic-light placeholder PNG using only the stdlib."""
import math
import struct
import sys
import zlib


def make_icon(size: int, path: str) -> None:
    bg = (13, 15, 19)          # #0d0f13 panel background
    red = (255, 74, 61)
    yellow = (255, 210, 76)
    green = (64, 217, 121)

    lamp_r = size / 6.5
    centers = [
        (size / 2, size * 0.245, red),
        (size / 2, size / 2, yellow),
        (size / 2, size * 0.755, green),
    ]
    margin = size * 0.08
    corner = size * 0.16

    raw = bytearray()
    for y in range(size):
        raw.append(0)  # PNG filter: None
        for x in range(size):
            r, g, b = bg
            cx = max(margin + corner, min(x, size - margin - corner))
            cy = max(margin + corner, min(y, size - margin - corner))
            corner_d = math.hypot(x - cx, y - cy)
            if (
                x < margin
                or x >= size - margin
                or y < margin
                or y >= size - margin
                or corner_d > corner
            ):
                raw.extend((8, 10, 14))
                continue

            for lx, ly, (lr, lg, lb) in centers:
                d = math.hypot(x - lx, y - ly)
                if d <= lamp_r:
                    t = d / lamp_r
                    glow = 1.0 - t * t
                    r = int(lr * (0.55 + 0.45 * glow))
                    g = int(lg * (0.55 + 0.45 * glow))
                    b = int(lb * (0.55 + 0.45 * glow))
                    if lamp_r - d < 2.0:
                        alpha = (lamp_r - d) / 2.0
                        r = int(r * alpha + bg[0] * (1 - alpha))
                        g = int(g * alpha + bg[1] * (1 - alpha))
                        b = int(b * alpha + bg[2] * (1 - alpha))
                    break
            raw.extend((max(0, min(255, r)), max(0, min(255, g)), max(0, min(255, b))))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    header = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0)
    idat = zlib.compress(bytes(raw), 9)
    with open(path, "wb") as fh:
        fh.write(header + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b""))


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "traffic-light-icon.png"
    make_icon(1024, out)
    print(f"wrote {out}")
