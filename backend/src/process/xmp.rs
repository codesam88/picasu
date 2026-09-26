use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Metadata extracted from a file's XMP packet or sidecar.
#[derive(Debug, Default)]
pub struct XmpData {
    pub tags: HashSet<String>,
    pub description: Option<String>,
    /// 0–5 per XMP `xmp:Rating`; -1 ("rejected") in some tools is clamped to None.
    pub rating: Option<u8>,
    /// `dc:title`. Used for the album display name override.
    pub title: Option<String>,
}

/// Extract XMP metadata from raw bytes (file contents or sidecar content).
///
/// Handles the fields the app manages:
/// - `dc:subject`   → tags (`rdf:Bag` of `rdf:li`)
/// - `dc:description` → description (`rdf:Alt` of `rdf:li`)
/// - `xmp:Rating`   → rating (plain integer text node)
/// - `dc:title`     → title (`rdf:Alt` of `rdf:li`)
///
/// # The scan is container-unaware
///
/// Nothing here parses a container. JPEG marker segments, PNG chunks, TIFF
/// IFDs — no structure is walked; the only thing recognised is a literal
/// `<element>…</element>` byte pair, and it is accepted anywhere in the input.
/// Three consequences follow, and the capability manifest's
/// `metadataFields.xmp` entries must be read in light of them:
///
/// 1. *Embedded* is a location, not a detection guarantee. A JPEG APP1 XMP
///    packet is found because it happens to be stored verbatim in the file
///    bytes. A packet serialized differently is not: compact XMP (namespace
///    shorthand, RDF attribute syntax, a `dcterms:`-style prefix) has no
///    literal `<dc:subject>`, so it yields no tags even though the file does
///    carry XMP. Treat `jpeg.xmp: ["embedded"]` as "an XMP packet in the usual
///    form is read", not "any XMP in a JPEG is read".
/// 2. The converse also holds: any bytes containing those markers match,
///    whether or not they are a well-formed packet inside the right segment.
///    There is no APP1 or namespace check to keep a stray match honest.
/// 3. Nothing is decompressed, so only plaintext survives. A PNG `zTXt`/`iTXt`
///    chunk holding a deflate-compressed packet yields nothing.
///
/// PNG embedded XMP is not a supported contract (`capabilities.json` lists
/// `xmp:embedded` under the PNG format's `unsupportedMetadataFields`) and
/// `pinned_png_compressed_embedded_xmp_is_not_extracted` keeps it that way. An
/// *uncompressed* PNG text chunk would be picked up by the byte scan, but that
/// is an accident of the scan above and must not be read as PNG support.
pub fn extract_xmp_data(bytes: &[u8]) -> XmpData {
    XmpData {
        tags: extract_bag_field(bytes, b"<dc:subject>", b"</dc:subject>"),
        description: extract_alt_text(bytes, b"<dc:description>", b"</dc:description>"),
        // Parse as i32 to handle negative values (e.g. -1 = "rejected"); clamp to None.
        rating: extract_simple_integer(bytes, b"<xmp:Rating>", b"</xmp:Rating>")
            .and_then(|v| u8::try_from(v).ok().filter(|&r| r <= 5)),
        title: extract_alt_text(bytes, b"<dc:title>", b"</dc:title>"),
    }
}

/// Return the `.xmp` sidecar path alongside `path` if it exists.
/// Convention: `photo.jpg` → `photo.xmp` (Adobe/Lightroom naming).
pub fn discover_sidecar(path: &Path) -> Option<PathBuf> {
    let sidecar = path.with_extension("xmp");
    if sidecar.exists() {
        Some(sidecar)
    } else {
        None
    }
}

/// Read `path` (or its `.xmp` sidecar if one exists) and extract XMP metadata.
/// Returns default empty data on read errors.
pub fn extract_xmp_data_from_file(path: &Path) -> XmpData {
    // Prefer sidecar over embedded: sidecar is the write-back target
    // (Area 3) so it is always authoritative when present.
    let source = discover_sidecar(path).unwrap_or_else(|| path.to_path_buf());
    match std::fs::read(&source) {
        Ok(bytes) => extract_xmp_data(&bytes),
        Err(_) => XmpData::default(),
    }
}

// ── Internals ─────────────────────────────────────────────────────────────────

/// Extract all `<rdf:li>` text children of `element_open..element_close`.
/// Used for `dc:subject` (Bag of keywords).
fn extract_bag_field(bytes: &[u8], open: &[u8], close: &[u8]) -> HashSet<String> {
    let mut result = HashSet::new();
    let Some(open_pos) = find_subslice(bytes, open) else {
        return result;
    };
    let inner_start = open_pos + open.len();
    let Some(close_offset) = find_subslice(&bytes[inner_start..], close) else {
        return result;
    };
    let inner = &bytes[inner_start..inner_start + close_offset];
    let Ok(inner_text) = std::str::from_utf8(inner) else {
        return result;
    };
    collect_rdf_li(inner_text, &mut result);
    result
}

/// Extract the first `<rdf:li>` text child of `element_open..element_close`.
/// Used for `dc:description` (Alt-text with language alternatives).
fn extract_alt_text(bytes: &[u8], open: &[u8], close: &[u8]) -> Option<String> {
    let open_pos = find_subslice(bytes, open)?;
    let inner_start = open_pos + open.len();
    let close_offset = find_subslice(&bytes[inner_start..], close)?;
    let inner = &bytes[inner_start..inner_start + close_offset];
    let inner_text = std::str::from_utf8(inner).ok()?;

    // Try rdf:Alt > rdf:li first
    let mut items = HashSet::new();
    collect_rdf_li(inner_text, &mut items);
    if let Some(item) = items.into_iter().next() {
        let trimmed = item.trim().to_owned();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    // Fallback: plain text content (e.g. <dc:description>text</dc:description>)
    let trimmed = inner_text.trim().to_owned();
    if !trimmed.is_empty() {
        return Some(trimmed);
    }
    None
}

/// Extract a plain integer from a simple text-node element.
/// Used for `xmp:Rating`.
fn extract_simple_integer(bytes: &[u8], open: &[u8], close: &[u8]) -> Option<i32> {
    let open_pos = find_subslice(bytes, open)?;
    let inner_start = open_pos + open.len();
    let close_offset = find_subslice(&bytes[inner_start..], close)?;
    let inner = &bytes[inner_start..inner_start + close_offset];
    let text = std::str::from_utf8(inner).ok()?.trim();
    text.parse::<i32>().ok()
}

/// Walk `<rdf:li ...>…</rdf:li>` entries in `text`, adding trimmed non-empty
/// values to `out`.
fn collect_rdf_li(text: &str, out: &mut HashSet<String>) {
    let mut rest = text;
    while let Some(li_start) = rest.find("<rdf:li") {
        let from_li = &rest[li_start..];
        let Some(tag_end) = from_li.find('>') else {
            break;
        };
        let content = &from_li[tag_end + 1..];
        let Some(li_end) = content.find("</rdf:li>") else {
            break;
        };
        let value = content[..li_end].trim();
        if !value.is_empty() {
            out.insert(value.to_owned());
        }
        rest = &content[li_end + "</rdf:li>".len()..];
    }
}

/// Find the first occurrence of `needle` in `haystack` (raw byte scan).
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xmp_full(keywords: &[&str], description: &str, rating: i32) -> String {
        let items: String = keywords
            .iter()
            .map(|k| format!("<rdf:li>{k}</rdf:li>"))
            .collect();
        let desc_items = format!("<rdf:li xml:lang=\"x-default\">{description}</rdf:li>");
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"
                 xmlns:xmp="http://ns.adobe.com/xap/1.0/">
<dc:subject><rdf:Bag>{items}</rdf:Bag></dc:subject>
<dc:description><rdf:Alt>{desc_items}</rdf:Alt></dc:description>
<xmp:Rating>{rating}</xmp:Rating>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>"#
        )
    }

    fn xmp_with_title(title: &str) -> String {
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:title><rdf:Alt><rdf:li xml:lang="x-default">{title}</rdf:li></rdf:Alt></dc:title>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>"#
        )
    }

    fn xmp_packet_with_keywords(keywords: &[&str]) -> String {
        let items: String = keywords
            .iter()
            .map(|k| format!("<rdf:li>{k}</rdf:li>"))
            .collect();
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:subject><rdf:Bag>{items}</rdf:Bag></dc:subject>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>"#
        )
    }

    #[test]
    fn extracts_all_fields() {
        let xmp = xmp_full(&["sunset", "travel"], "A beautiful sunset", 4);
        let data = extract_xmp_data(xmp.as_bytes());
        assert_eq!(
            data.tags,
            HashSet::from(["sunset".to_string(), "travel".to_string()])
        );
        assert_eq!(data.description.as_deref(), Some("A beautiful sunset"));
        assert_eq!(data.rating, Some(4));
    }

    #[test]
    fn rating_out_of_range_is_none() {
        let xmp = xmp_full(&[], "", 6);
        let data = extract_xmp_data(xmp.as_bytes());
        assert_eq!(data.rating, None);
    }

    #[test]
    fn missing_fields_are_empty_or_none() {
        let xmp = xmp_packet_with_keywords(&["family"]);
        let data = extract_xmp_data(xmp.as_bytes());
        assert_eq!(data.tags, HashSet::from(["family".to_string()]));
        assert_eq!(data.description, None);
        assert_eq!(data.rating, None);
    }

    #[test]
    fn extracts_keywords_from_dc_subject_bag() {
        let xmp = xmp_packet_with_keywords(&["family", "vacation"]);
        let data = extract_xmp_data(xmp.as_bytes());
        assert_eq!(
            data.tags,
            HashSet::from(["family".to_string(), "vacation".to_string()])
        );
    }

    #[test]
    fn finds_packet_embedded_inside_arbitrary_container_bytes() {
        let xmp = xmp_packet_with_keywords(&["sunset"]);
        let mut bytes = b"\xff\xd8\xff\xe0JFIF garbage binary prefix".to_vec();
        bytes.extend_from_slice(xmp.as_bytes());
        bytes.extend_from_slice(b"more binary jpeg scan data\xff\xd9");
        let data = extract_xmp_data(&bytes);
        assert_eq!(data.tags, HashSet::from(["sunset".to_string()]));
    }

    #[test]
    fn returns_empty_set_when_no_xmp_packet_present() {
        let data = extract_xmp_data(b"\xff\xd8\xff plain jpeg, no xmp");
        assert!(data.tags.is_empty());
    }

    #[test]
    fn returns_empty_set_when_dc_subject_is_absent_or_empty() {
        let xmp = xmp_packet_with_keywords(&[]);
        let data = extract_xmp_data(xmp.as_bytes());
        assert!(data.tags.is_empty());
    }

    #[test]
    fn extracts_title() {
        let xmp = xmp_with_title("My Album");
        let data = extract_xmp_data(xmp.as_bytes());
        assert_eq!(data.title.as_deref(), Some("My Album"));
    }

    #[test]
    fn title_is_none_when_absent() {
        let xmp = xmp_packet_with_keywords(&["family"]);
        let data = extract_xmp_data(xmp.as_bytes());
        assert_eq!(data.title, None);
    }

    /// PNG embedded XMP is not a supported contract, and the reason is pinned
    /// here rather than left implicit: a PNG carries its XMP in a text chunk,
    /// which may be deflate-compressed, and this extractor decompresses
    /// nothing. A packet that is compressed is invisible; nothing about PNG
    /// indexing may start depending on it.
    #[test]
    fn pinned_png_compressed_embedded_xmp_is_not_extracted() {
        let xmp = xmp_packet_with_keywords(&["e2e_png_embedded"]);
        // Guard against a vacuous fixture: the same packet in plaintext is
        // readable, so only the compression can be hiding it.
        assert_eq!(
            extract_xmp_data(xmp.as_bytes()).tags,
            HashSet::from(["e2e_png_embedded".to_string()])
        );

        let png = png_with_ztxt_chunk(b"XML:com.adobe.xmp", &zlib_fixed_huffman(xmp.as_bytes()));
        assert!(
            !png.windows(b"<dc:subject>".len())
                .any(|w| w == b"<dc:subject>"),
            "fixture must not leave the packet in plaintext"
        );

        let data = extract_xmp_data(&png);
        assert_eq!(data.tags, HashSet::new());
        assert_eq!(data.description, None);
        assert_eq!(data.rating, None);
        assert_eq!(data.title, None);
    }

    /// PNG bytes carrying an XMP packet in a `zTXt` chunk: chunk type,
    /// keyword, null separator, compression method, deflate payload. The rest
    /// of the stream is filler — the point is the shape of the metadata chunk,
    /// not a decodable image.
    fn png_with_ztxt_chunk(keyword: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"zTXt");
        data.extend_from_slice(keyword);
        data.push(0x00); // keyword terminator
        data.push(0x00); // compression method: deflate
        data.extend_from_slice(payload);

        let mut chunk = Vec::new();
        chunk.extend_from_slice(
            &u32::try_from(data.len())
                .expect("ztxt chunk too large")
                .to_be_bytes(),
        );
        chunk.extend_from_slice(&data);
        chunk.extend_from_slice(&[0, 0, 0, 0]); // crc, not read by anything below

        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&chunk);
        png.extend_from_slice(b"trailing image data");
        png
    }

    /// `data` as a zlib stream holding one fixed-Huffman DEFLATE block.
    ///
    /// Literals only: it does not shrink anything, but it is a genuine deflate
    /// stream that any inflater accepts. A stored (uncompressed) block would
    /// not do — the plaintext would survive in the fixture, and the test above
    /// would then pass for the wrong reason. Cross-checked against
    /// `zlib.decompress`; no compression dependency is wanted here.
    fn zlib_fixed_huffman(data: &[u8]) -> Vec<u8> {
        // CMF 0x78 = deflate with a 32K window, FLG 0x01 makes the 16-bit
        // header a multiple of 31 as zlib requires.
        let mut bits = BitWriter::new(vec![0x78, 0x01]);
        bits.push(0b011, 3); // BFINAL=1, BTYPE=fixed Huffman
        for byte in data {
            // Fixed literal code: 8 bits up to 0x8f, 9 bits above.
            let (code, len) = if *byte < 144 {
                (0x30 + u32::from(*byte), 8)
            } else {
                (0x190 + u32::from(*byte) - 144, 9)
            };
            bits.push(reverse(code, len), len);
        }
        bits.push(0, 7); // end of block
        bits.finish();

        let (mut a, mut b) = (1u32, 0u32); // Adler-32
        for byte in data {
            a = (a + u32::from(*byte)) % 65521;
            b = (b + a) % 65521;
        }
        bits.out.extend_from_slice(&(((b << 16) | a).to_be_bytes()));
        bits.out
    }

    /// DEFLATE packs bits LSB-first, so a Huffman code (defined MSB-first) has
    /// to be reversed before it is written.
    fn reverse(mut code: u32, len: u32) -> u32 {
        let mut reversed = 0;
        for _ in 0..len {
            reversed = (reversed << 1) | (code & 1);
            code >>= 1;
        }
        reversed
    }

    struct BitWriter {
        out: Vec<u8>,
        acc: u32,
        pending: u32,
    }

    impl BitWriter {
        fn new(out: Vec<u8>) -> Self {
            Self {
                out,
                acc: 0,
                pending: 0,
            }
        }

        fn push(&mut self, bits: u32, len: u32) {
            self.acc |= bits << self.pending;
            self.pending += len;
            while self.pending >= 8 {
                self.out.push((self.acc & 0xff) as u8);
                self.acc >>= 8;
                self.pending -= 8;
            }
        }

        /// Flush the last partial byte, zero-padded as deflate requires.
        fn finish(&mut self) {
            if self.pending > 0 {
                self.out.push((self.acc & 0xff) as u8);
                self.acc = 0;
                self.pending = 0;
            }
        }
    }
}
