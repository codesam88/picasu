#!/usr/bin/env python3
"""Write the bare 48x32 PNG that `picasu-png-48x32-xmp-plain.png` is built from.

Run from the repository root in two steps:

    python3 utils/snapfab/fixtures/png/generate_picasu-png-48x32-xmp-plain.py \
        utils/snapfab/fixtures/png/picasu-png-48x32-xmp-plain.png
    exiftool -overwrite_original -q \
        -XMP-dc:Subject+=e2e_png_plain_alpine \
        -XMP-dc:Subject+=e2e_png_plain_winter \
        -XMP-dc:Description='A caption from the plain iTXt packet' \
        -XMP-xmp:Rating=4 \
        utils/snapfab/fixtures/png/picasu-png-48x32-xmp-plain.png

and compare the SHA-256 with the one `utils/snapfab/capabilities.json` records.

The script writes only IHDR, IDAT and IEND — no text chunks, no XMP — so the
packet in the final file is whatever ExifTool writes, and ExifTool 13.59 writes
a PNG's XMP into an *uncompressed* `iTXt` chunk (measured: the compression flag
after the `XML:com.adobe.xmp` keyword is 0). That uncompressed form is the one
xmpkit reads (Iteration 0 of `.plan/exif-xmp-rs-engine.md`), which makes this
the fixture behind the manifest's `png xmp: [embedded]` claim; the sibling
`picasu-png-48x32-xmp-text.png` carries the compressed form, which xmpkit
refuses and the manifest records as not covered.

The image body is the same 48x32 RGB buffer as the TIFF and WebP fixtures
(R = x*255/47, G = y*255/31, B = 128), so the pinned still images show the
same picture and a decoded-dimension assertion means the same thing in all of
them.
"""

from __future__ import annotations

import struct
import sys
import zlib
from pathlib import Path

WIDTH = 48
HEIGHT = 32


def chunk(kind: bytes, data: bytes) -> bytes:
    """One PNG chunk: length, type, data, CRC over type and data."""
    crc = zlib.crc32(kind + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", crc)


def scanlines() -> bytes:
    """The image body: one filter byte (0, none) and RGB per row."""
    rows = []
    for y in range(HEIGHT):
        row = bytearray(b"\x00")
        for x in range(WIDTH):
            row += bytes((x * 255 // 47, y * 255 // 31, 128))
        rows.append(bytes(row))
    return b"".join(rows)


def deflate(data: bytes) -> bytes:
    """`data` as a zlib stream, with every parameter pinned.

    The defaults are stated rather than inherited so a re-derivation on another
    interpreter is explicit about what has to match: the deflate output of a
    given zlib version at these parameters. The recorded SHA-256 in
    `capabilities.json` is what a re-derivation is checked against.
    """
    compressor = zlib.compressobj(9, zlib.DEFLATED, 15, 8, zlib.Z_DEFAULT_STRATEGY)
    return compressor.compress(data) + compressor.flush()


def build() -> bytes:
    ihdr = struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", deflate(scanlines()))
        + chunk(b"IEND", b"")
    )


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(f"usage: {argv[0]} <output.png>", file=sys.stderr)
        return 2
    target = Path(argv[1])
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(build())
    print(f"wrote {len(target.read_bytes())} bytes to {target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
