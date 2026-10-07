#!/usr/bin/env python3
"""Exercise the real offscreen icon CLI with original artwork; no GUI or vendor art.

Requires macOS and its Swift compiler. Uses only Python's standard library.
Temporary images, executable, and outputs stay in one owned ignored .build folder.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import tempfile
import time
import zlib


PROJECT = Path(__file__).resolve().parent.parent
SOURCE = PROJECT / "runtime-patches" / "native_icon.swift"
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def chunk(kind: bytes, value: bytes) -> bytes:
    return struct.pack(">I", len(value)) + kind + value + struct.pack(">I", zlib.crc32(kind + value))


def png(width: int, height: int, pixel) -> bytes:
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            rows.extend(pixel(x, y))
    return PNG_SIGNATURE + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) + \
        chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


def decode_png(data: bytes):
    assert data.startswith(PNG_SIGNATURE)
    offset, payload, dimensions = 8, bytearray(), None
    while offset < len(data):
        length = struct.unpack_from(">I", data, offset)[0]
        kind, value = data[offset + 4:offset + 8], data[offset + 8:offset + 8 + length]
        if kind == b"IHDR":
            width, height, bits, colour, compression, filtering, interlace = struct.unpack(">IIBBBBB", value)
            assert bits == 8 and colour in (2, 6) and (compression, filtering, interlace) == (0, 0, 0)
            dimensions = width, height, 4 if colour == 6 else 3
        elif kind == b"IDAT":
            payload.extend(value)
        offset += length + 12
    assert dimensions is not None
    width, height, channels = dimensions
    raw = zlib.decompress(payload)
    stride = width * channels
    assert len(raw) == height * (stride + 1)
    previous = bytearray(stride)
    pixels = bytearray()
    for y in range(height):
        start = y * (stride + 1)
        filter_type = raw[start]
        row = bytearray(raw[start + 1:start + stride + 1])
        for x in range(stride):
            left = row[x - channels] if x >= channels else 0
            above = previous[x]
            upper_left = previous[x - channels] if x >= channels else 0
            if filter_type == 1:
                prediction = left
            elif filter_type == 2:
                prediction = above
            elif filter_type == 3:
                prediction = (left + above) // 2
            elif filter_type == 4:
                p = left + above - upper_left
                distances = abs(p - left), abs(p - above), abs(p - upper_left)
                prediction = (left, above, upper_left)[distances.index(min(distances))]
            else:
                assert filter_type == 0
                prediction = 0
            row[x] = (row[x] + prediction) & 255
        for x in range(0, stride, channels):
            pixels.extend(row[x:x + 3])
            pixels.append(row[x + 3] if channels == 4 else 255)
        previous = row
    return width, height, pixels


def representation(data: bytes, requested: bytes) -> bytes:
    assert data[:4] == b"icns" and struct.unpack_from(">I", data, 4)[0] == len(data)
    offset = 8
    expected = {b"icp4": 16, b"icp5": 32, b"icp6": 64, b"ic07": 128, b"ic08": 256,
                b"ic09": 512, b"ic10": 1024, b"ic11": 32, b"ic12": 64, b"ic13": 256, b"ic14": 512}
    found = None
    seen = set()
    while offset < len(data):
        kind, length = data[offset:offset + 4], struct.unpack_from(">I", data, offset + 4)[0]
        value = data[offset + 8:offset + length]
        assert kind in expected and length > 8 and offset + length <= len(data)
        assert value.startswith(PNG_SIGNATURE)
        assert struct.unpack_from(">II", value, 16) == (expected[kind], expected[kind])
        seen.add(kind)
        if kind == requested:
            found = value
        offset += length
    assert offset == len(data) and seen == set(expected) and found is not None
    return found


def at(image, x: int, y: int):
    width, height, pixels = image
    assert 0 <= x < width and 0 <= y < height
    start = (y * width + x) * 4
    return tuple(pixels[start:start + 4])


def rounded(x: int, y: int, low: int, high: int, radius: float) -> bool:
    if not low <= x < high or not low <= y < high:
        return False
    cx = min(max(x + 0.5, low + radius), high - radius)
    cy = min(max(y + 0.5, low + radius), high - radius)
    return (x + 0.5 - cx) ** 2 + (y + 0.5 - cy) ** 2 <= radius ** 2


def main() -> None:
    if os.uname().sysname != "Darwin":
        raise SystemExit("This renderer test requires macOS.")
    build = PROJECT / ".build"
    build.mkdir(exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="adaptive-icon-test-", dir=build))
    directory.chmod(0o700)
    identity = directory.stat().st_dev, directory.stat().st_ino
    (directory / ".owner.json").write_text(json.dumps({"purpose": "original-icon-fixtures", "uid": os.getuid(),
                                                        "identity": identity}))
    renderer = directory / "native-icon"
    timings = []
    checks = 0
    try:
        subprocess.run(["/usr/bin/swiftc", "-O", "-parse-as-library", "-target", "arm64-apple-macos13", str(SOURCE), "-o", str(renderer)],
                       check=True, capture_output=True, timeout=120)

        def convert(name, data, success=True):
            nonlocal checks
            input_path = directory / f"{name}.png"
            input_path.write_bytes(data)
            output = directory / name
            output.mkdir(mode=0o700)
            started = time.monotonic()
            result = subprocess.run([str(renderer), "--input", str(input_path), "--output", str(output / "app.icns"),
                                     "--png-output", str(output / "app.png")], capture_output=True, timeout=1.1)
            duration = time.monotonic() - started
            assert (result.returncode == 0) == success, (name, result.stderr.decode(errors="replace"))
            checks += 1
            if not success:
                assert not list(output.iterdir())
                return None
            timings.append(duration)
            assert struct.unpack_from(">II", (output / "app.png").read_bytes(), 16) == (1024, 1024)
            return decode_png(representation((output / "app.icns").read_bytes(), b"ic08"))

        transparent = (0, 0, 0, 0)
        blue, yellow, white = (14, 82, 216, 255), (246, 209, 30, 255), (255, 255, 255, 255)
        tile = convert("finished-tile", png(256, 256, lambda x, y:
                       yellow if 98 <= x <= 156 and 93 <= y <= 161 else
                       blue if rounded(x, y, 24, 232, 87) else transparent))
        assert at(tile, 37, 37)[3] < 10, "A completed tile acquired an extra surrounding plate"
        assert at(tile, 128, 22)[:3] == blue[:3] and at(tile, 128, 128)[:3] == yellow[:3]

        circle = convert("circle", png(256, 256, lambda x, y:
                         white if 114 <= x <= 142 and 70 <= y <= 186 else
                         blue if (x - 128) ** 2 + (y - 128) ** 2 <= 104 ** 2 else transparent))
        assert at(circle, 37, 37)[3] < 10, "An intentional circle acquired a square background"
        assert at(circle, 128, 25)[3] > 250 and at(circle, 128, 128)[:3] == white[:3]
        black_circle = convert("black-circle", png(256, 256, lambda x, y:
                               (0, 0, 0, 255) if (x - 128) ** 2 + (y - 128) ** 2 <= 104 ** 2 else transparent))
        assert at(black_circle, 37, 37)[3] < 10 and at(black_circle, 128, 128) == (0, 0, 0, 255)

        for name, colour, brightness in (("white-mark", white, "dark"), ("black-mark", (0, 0, 0, 255), "light"),
                                         ("bright-yellow-mark", (255, 240, 32, 255), "dark")):
            mark = convert(name, png(256, 256, lambda x, y:
                           colour if (96 <= x < 114 and 60 <= y < 196) or
                           (96 <= x < 187 and 176 <= y < 196) else transparent))
            support = at(mark, 128, 50)
            assert support[3] == 255
            assert max(support[:3]) < 80 if brightness == "dark" else min(support[:3]) > 210
            visible = [at(mark, x, y) for y in range(256) for x in range(256)]
            assert any(pixel[:3] == colour[:3] and pixel[3] == 255 for pixel in visible)

        pale_multicolour = convert("pale-multicolour-mark", png(256, 256, lambda x, y:
                                 (255, 240, 32, 255) if 96 <= x < 114 and 60 <= y < 196 else
                                 (144, 255, 90, 255) if 96 <= x < 187 and 176 <= y < 196 else transparent))
        assert at(pale_multicolour, 128, 50)[3] < 10, "Distinct bright colours were mistaken for one faint glyph"
        assert any(at(pale_multicolour, x, y) == (255, 240, 32, 255) for y in range(256) for x in range(256))
        assert any(at(pale_multicolour, x, y) == (144, 255, 90, 255) for y in range(256) for x in range(256))

        def coloured_shape(x, y):
            x, y = x - 96, y - 96
            if 0 <= x < 64 and 0 <= y < 64:
                if abs(x - y) < 10:
                    return (235, 35, 142, 255)
                if abs((63 - x) - y) < 10:
                    return (20, 209, 195, 255)
            return transparent

        padded = convert("padded-colour-art", png(256, 256, coloured_shape))
        occupied = [(x, y) for y in range(256) for x in range(256) if at(padded, x, y)[3] > 100]
        assert max(x for x, _ in occupied) - min(x for x, _ in occupied) > 200
        assert at(padded, 128, 50)[3] < 10, "A strong silhouette acquired a plate"

        square = convert("opaque-square", png(256, 256, lambda x, y:
                         yellow if 90 <= x <= 166 and 90 <= y <= 166 else blue))
        assert at(square, 37, 37)[:3] == blue[:3] and at(square, 37, 37)[3] == 255
        assert at(square, 12, 12)[3] < 10 and at(square, 128, 128)[:3] == yellow[:3]

        # Asymmetric artwork catches vertically reversed crop bounds or colour edits.
        asymmetric = convert("offset-art", png(256, 256, lambda x, y:
                             (230, 23, 90, 255) if 40 <= x < 80 and 16 <= y < 76 else
                             (18, 180, 190, 255) if 40 <= x < 112 and 76 <= y < 96 else transparent))
        assert at(asymmetric, 128, 57)[:3] == (230, 23, 90)
        assert at(asymmetric, 128, 211)[:3] == (18, 180, 190)

        ico_png = png(256, 256, lambda x, y: blue if (x - 128) ** 2 + (y - 128) ** 2 <= 104 ** 2 else transparent)
        ico = b"\x00\x00\x01\x00\x01\x00" + struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(ico_png), 22) + ico_png
        ico_image = convert("ico", ico)
        assert at(ico_image, 37, 37)[3] < 10 and at(ico_image, 128, 128)[:3] == blue[:3]

        convert("transparent", png(64, 64, lambda x, y: transparent), success=False)
        convert("invalid", PNG_SIGNATURE + b"not an image", success=False)
        convert("oversized", b"a" * (16 * 1024 * 1024 + 1), success=False)
        convert("oversized-dimensions", png(4097, 1, lambda x, y: blue), success=False)

        valid_input = directory / "opaque-square.png"
        victim = directory / "untouched"
        victim.write_bytes(b"untouched")

        def reject(name, input_path=valid_input, prepare=None, path_transform=None):
            nonlocal checks
            output = directory / name
            output.mkdir(mode=0o700)
            if prepare:
                prepare(output)
            output_path = path_transform(output) if path_transform else output
            result = subprocess.run([str(renderer), "--input", str(input_path), "--output", str(output_path / "app.icns"),
                                     "--png-output", str(output_path / "app.png")], capture_output=True, timeout=1.1)
            assert result.returncode != 0, name
            assert victim.read_bytes() == b"untouched"
            checks += 1
            return output

        link_input = directory / "input-link.png"
        link_input.symlink_to(valid_input)
        reject("input-symlink", link_input)
        parent_link = directory / "directory-link"
        parent_link.symlink_to(directory, target_is_directory=True)
        reject("parent-symlink", parent_link / valid_input.name)
        fifo = directory / "input-fifo.png"
        os.mkfifo(fifo, 0o600)
        reject("input-fifo", fifo)
        reject("output-symlink", prepare=lambda output: (output / "app.icns").symlink_to(victim))
        existing = reject("existing-png", prepare=lambda output: (output / "app.png").write_bytes(b"original"))
        assert (existing / "app.png").read_bytes() == b"original" and not (existing / "app.icns").exists()
        reject("writable-output", prepare=lambda output: output.chmod(0o777))
        reject("noncanonical-output", path_transform=lambda output: output / ".." / output.name)
        target = directory / "linked-output-target"
        target.mkdir(mode=0o700)
        reject("linked-output", path_transform=lambda output: parent_link / target.name)

        assert timings and max(timings) < 1.1
        print(f"Passed {checks} real CLI cases; slowest render {max(timings):.3f}s (1.1s budget).")
    finally:
        metadata = directory.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid() or \
                (metadata.st_dev, metadata.st_ino) != identity:
            raise RuntimeError("Refusing to clean a replaced test directory")
        # The only processes created above are bounded subprocess.run calls,
        # already waited or killed-and-waited on timeout. Also refuse foreign open owners.
        owners = subprocess.run(["/usr/sbin/lsof", "+D", str(directory)], capture_output=True, timeout=5)
        if owners.returncode not in (0, 1) or owners.stdout:
            raise RuntimeError("Refusing cleanup while fixture files have open owners")
        shutil.rmtree(directory)


if __name__ == "__main__":
    main()
