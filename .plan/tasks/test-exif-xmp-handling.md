---
status: open
type: feature
priority: medium
area: testing
---

Enable metadata-extraction tests for all supported file formats.

`extract_keywords_from_xmp`'s current (stub) substring-scan approach is format-agnostic but only `scenario_n` (JPEG) exercises it end-to-end. XMP/IPTC packet location and encoding differ by container — PNG (zTXt chunks), MP4/MOV (uuid box), TIFF (no APPn), IPTC IIM (separate binary format).

Once real extraction is implemented, extend coverage with one test per representative container (PNG iTXt, PNG zTXt, MP4 uuid XMP, IPTC-IIM-only JPEG). Video-pipeline coverage additionally needs `ffmpeg`/`ffprobe` available in the test environment.

2026-06-30: Real XMP extraction now implemented (hand-written byte-scan in xmp.rs, 7 unit tests). The non-JPEG container gap remains open.
