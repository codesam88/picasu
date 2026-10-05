#!/usr/bin/env python3
"""Write the checked-in PNG fixture that carries a compressed `iTXt` XMP packet.

Run from the repository root:

    python3 utils/snapfab/fixtures/png/generate_picasu-png-48x32-xmp-text.py \
        utils/snapfab/fixtures/png/picasu-png-48x32-xmp-text.png

and compare the SHA-256 with the one `utils/snapfab/capabilities.json` records.
The script writes the bytes itself rather than wrapping an encoder, so the file
depends only on this source and the standard library — the same argument the
TIFF fixture's provenance makes.

Why a script and not ExifTool: ExifTool writes an XMP packet into a PNG as an
*uncompressed* `iTXt` chunk (measured on ExifTool 13.59: the compression flag
after the `XML:com.adobe.xmp` keyword is 0), and it does not write `tEXt` chunks
of its own choosing. The compressed form is the case the capability claim is
about, so the fixture is built here instead.

The image body is the same 48x32 RGB buffer as the TIFF and WebP fixtures
(R = x*255/47, G = y*255/31, B = 128), so the three pinned still images show the
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

# The XMP packet, carried deflate-compressed in an `iTXt` chunk. `dc:subject`
# and `dc:description` are the two fields the backend maps natively, `dc:title`
# and `xmp:Rating` the other two, so one packet exercises every native field
# that has an XMP source.
XMP_PACKET = b"""<?xpacket begin="\xef\xbb\xbf" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about=""
  xmlns:dc="http://purl.org/dc/elements/1.1/"
  xmlns:xmp="http://ns.adobe.com/xap/1.0/">
<dc:title><rdf:Alt><rdf:li xml:lang="x-default">A title from the compressed packet</rdf:li></rdf:Alt></dc:title>
<dc:subject><rdf:Bag>
<rdf:li>e2e_png_xmp_alpine</rdf:li>
<rdf:li>e2e_png_xmp_winter</rdf:li>
</rdf:Bag></dc:subject>
<dc:description><rdf:Alt><rdf:li xml:lang="x-default">A caption from the compressed packet</rdf:li></rdf:Alt></dc:description>
<xmp:Rating>4</xmp:Rating>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"""

# `tEXt` chunks: keyword, Latin-1 text. `Description` is one of the two keywords
# the native mapping reads from a PNG, which makes this file a precedence case:
# the packet's own `dc:description` outranks the text chunk. `Comment` and
# `Source` are not mapped natively, so they are what the further-data bucket
# gets.
TEXT_CHUNKS = [
    (b"Description", b"A text chunk description"),
    (b"Comment", b"A text chunk comment"),
    (b"Source", b"picasu test fixtures"),
]

XMP_KEYWORD = b"XML:com.adobe.xmp"


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
    `capabilities.json` is what a re-derivation is checked against, and
    `version` there names the zlib that produced the checked-in bytes.
    """
    compressor = zlib.compressobj(9, zlib.DEFLATED, 15, 8, zlib.Z_DEFAULT_STRATEGY)
    return compressor.compress(data) + compressor.flush()


def text_chunk(keyword: bytes, text: bytes) -> bytes:
    """A `tEXt` chunk: keyword, null separator, Latin-1 text."""
    return chunk(b"tEXt", keyword + b"\x00" + text)


def compressed_xmp_chunk(packet: bytes) -> bytes:
    """An `iTXt` chunk holding a deflate-compressed packet.

    The layout is the one the PNG specification defines for an international
    text chunk: keyword, null, compression flag 1, compression method 0, empty
    language tag, empty translated keyword, null, then the compressed text.
    """
    data = XMP_KEYWORD + b"\x00" + bytes((1, 0)) + b"\x00\x00" + deflate(packet)
    return chunk(b"iTXt", data)


def build() -> bytes:
    ihdr = struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 2, 0, 0, 0)
    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
    for keyword, text in TEXT_CHUNKS:
        out += text_chunk(keyword, text)
    out += compressed_xmp_chunk(XMP_PACKET)
    out += chunk(b"IDAT", deflate(scanlines()))
    out += chunk(b"IEND", b"")
    return out


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
