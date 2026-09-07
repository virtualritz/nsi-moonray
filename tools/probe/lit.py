"""The lit rectangle in a PNG, so a render can be measured rather than looked at.

Reads the file with the standard library only -- 3Delight writes 8-bit
RGB, and pulling in an image library to find a bounding box would be a
dependency for nothing.
"""

import struct
import sys
import zlib


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", f"{path} is not a PNG"

    at, idat, width, height, depth, colour = 8, b"", None, None, None, None
    while at < len(data):
        length = struct.unpack(">I", data[at : at + 4])[0]
        kind = data[at + 4 : at + 8]
        chunk = data[at + 8 : at + 8 + length]
        at += 12 + length
        if kind == b"IHDR":
            width, height, depth, colour = struct.unpack(">IIBB", chunk[:10])
        elif kind == b"IDAT":
            idat += chunk
        elif kind == b"IEND":
            break

    assert depth == 8, f"{path}: {depth}-bit, expected 8"
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[colour]
    raw = zlib.decompress(idat)
    stride = width * channels

    # Undo the per-scanline filters. See PNG, section 9.
    out, previous, at = bytearray(), bytearray(stride), 0
    for _ in range(height):
        filt, at = raw[at], at + 1
        line, at = bytearray(raw[at : at + stride]), at + stride
        for x in range(stride):
            a = line[x - channels] if x >= channels else 0
            b = previous[x]
            c = previous[x - channels] if x >= channels else 0
            if filt == 1:
                line[x] = (line[x] + a) & 255
            elif filt == 2:
                line[x] = (line[x] + b) & 255
            elif filt == 3:
                line[x] = (line[x] + ((a + b) >> 1)) & 255
            elif filt == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                near = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + near) & 255
        out += line
        previous = line

    return width, height, channels, bytes(out)


def lit(path, threshold=127):
    width, height, channels, pixels = read_png(path)
    xs, ys = [], []
    for y in range(height):
        for x in range(width):
            if pixels[(y * width + x) * channels] > threshold:
                xs.append(x)
                ys.append(y)
    if not xs:
        return width, height, None
    return width, height, (min(xs), max(xs), min(ys), max(ys))


if __name__ == "__main__":
    for path in sys.argv[1:]:
        width, height, box = lit(path)
        if box is None:
            print(f"{path}: {width}x{height}, nothing lit")
        else:
            left, right, top, bottom = box
            print(
                f"{path}: {width}x{height}"
                f"  x {left}..{right} ({right - left + 1})"
                f"  y {top}..{bottom} ({bottom - top + 1})"
            )
