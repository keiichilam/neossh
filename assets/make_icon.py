"""Generate the NEO-SSH application icon: assets/neo-ssh.ico and assets/neo-ssh.png.

Standard library only. Every icon size is drawn from the vector description
at that size (small sizes get heavier strokes and pixel-aligned edges) rather
than downscaled from the large image.

    python assets/make_icon.py

build.rs embeds neo-ssh.ico in the Windows executable; src/desktop.rs uses
neo-ssh.png as the window and taskbar icon.
"""
import math
import pathlib
import struct
import zlib

ASSETS = pathlib.Path(__file__).resolve().parent
ICO_SIZES = (16, 20, 24, 32, 40, 48, 64, 256)

# Palette from src/desktop.rs (BACKGROUND, ACCENT, INK) plus a lighter navy.
TILE_TOP = (31, 45, 64)
TILE_BOTTOM = (13, 19, 28)
ACCENT = (83, 210, 185)
INK = (218, 227, 237)


def segment_distance(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def rounded_rect_distance(px, py, left, top, right, bottom, radius):
    cx, cy = (left + right) / 2, (top + bottom) / 2
    qx = abs(px - cx) - ((right - left) / 2 - radius)
    qy = abs(py - cy) - ((bottom - top) / 2 - radius)
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    return outside + min(max(qx, qy), 0.0) - radius


def coverage(distance):
    """Area coverage of a pixel whose centre is `distance` pixels outside an edge."""
    return max(0.0, min(1.0, 0.5 - distance))


def render(size):
    """Straight-alpha RGBA rows for a size x size icon."""
    unit = size / 256.0
    small = size <= 48

    def snap(value):
        return float(round(value)) if small else value

    # Tile: full-bleed at tiny sizes so it reads as a solid square.
    margin = 0.0 if size <= 24 else 8 * unit
    tile = (margin, margin, size - margin, size - margin, max(2.0, 46 * unit))
    rim = max(1.0, 5 * unit)

    # Prompt chevron ">".
    stroke = max(2.0, 26 * unit)
    chevron = ((70 * unit, 84 * unit), (128 * unit, 128 * unit), (70 * unit, 172 * unit))

    # Block cursor, its bottom level with the chevron's lower stroke end.
    cursor_bottom = snap(172 * unit + stroke / 2)
    cursor_height = max(2.0, round(26 * unit) if small else 26 * unit)
    cursor = (
        snap(150 * unit),
        cursor_bottom - cursor_height,
        snap(200 * unit),
        cursor_bottom,
        min(4 * unit, cursor_height / 2),
    )

    rows = []
    for y in range(size):
        row = []
        for x in range(size):
            px, py = x + 0.5, y + 0.5
            # Premultiplied accumulation, painted back to front.
            r = g = b = a = 0.0

            def over(color, alpha):
                nonlocal r, g, b, a
                r = color[0] * alpha + r * (1 - alpha)
                g = color[1] * alpha + g * (1 - alpha)
                b = color[2] * alpha + b * (1 - alpha)
                a = alpha + a * (1 - alpha)

            tile_distance = rounded_rect_distance(px, py, *tile)
            tile_alpha = coverage(tile_distance)
            if tile_alpha == 0.0:
                row.append((0, 0, 0, 0))
                continue
            t = py / size
            over(tuple(top + (bottom - top) * t for top, bottom in zip(TILE_TOP, TILE_BOTTOM)), tile_alpha)
            # Mint rim keeps the dark tile visible on a dark taskbar.
            ring = abs(tile_distance + rim / 2) - rim / 2
            over(ACCENT, 0.55 * min(coverage(ring), tile_alpha))
            arm = min(
                segment_distance(px, py, *chevron[0], *chevron[1]),
                segment_distance(px, py, *chevron[1], *chevron[2]),
            )
            over(ACCENT, coverage(arm - stroke / 2))
            over(INK, coverage(rounded_rect_distance(px, py, *cursor)))
            if a <= 0.0:
                row.append((0, 0, 0, 0))
            else:
                row.append(tuple(round(min(255.0, c / a)) for c in (r, g, b)) + (round(a * 255),))
        rows.append(row)
    return rows


def png(rows):
    height, width = len(rows), len(rows[0])
    raw = b"".join(b"\x00" + bytes(channel for pixel in row for channel in pixel) for row in rows)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def dib(rows):
    """32-bit BMP icon image (BGRA, bottom-up) with its 1-bit transparency mask."""
    height, width = len(rows), len(rows[0])
    colors = b"".join(bytes((p[2], p[1], p[0], p[3])) for row in reversed(rows) for p in row)
    stride = (width + 31) // 32 * 4
    mask = bytearray()
    for row in reversed(rows):
        line = bytearray(stride)
        for x, pixel in enumerate(row):
            if pixel[3] == 0:
                line[x // 8] |= 0x80 >> (x % 8)
        mask += line
    header = struct.pack("<IiiHHIIiiII", 40, width, height * 2, 1, 32, 0, len(colors) + len(mask), 0, 0, 0, 0)
    return header + colors + bytes(mask)


def ico(images):
    """images: (size, encoded image) pairs, smallest first."""
    directory = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    data = b""
    for size, image in images:
        directory += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(image), offset)
        offset += len(image)
        data += image
    return directory + data


def main():
    images = []
    for size in ICO_SIZES:
        rows = render(size)
        # PNG for 256 px keeps the file small; BMP for the rest suits every icon reader.
        images.append((size, png(rows) if size == 256 else dib(rows)))
    (ASSETS / "neo-ssh.ico").write_bytes(ico(images))
    (ASSETS / "neo-ssh.png").write_bytes(images[-1][1])
    print(f"Wrote {ASSETS / 'neo-ssh.ico'} ({', '.join(str(s) for s in ICO_SIZES)} px) and {ASSETS / 'neo-ssh.png'}")


if __name__ == "__main__":
    main()
