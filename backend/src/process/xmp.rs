use std::collections::HashSet;
use std::path::{Path, PathBuf};
use xmpkit::{XmpFile, XmpMeta, XmpValue, ns};

/// Metadata extracted from a file's XMP packet or sidecar.
#[derive(Debug, Default)]
pub struct XmpData {
    pub tags: HashSet<String>,
    pub description: Option<String>,
    /// 0–5 per XMP `xmp:Rating`; -1 ("rejected") in some tools is clamped to None.
    pub rating: Option<u8>,
    /// `dc:title`. Used for the album display name override.
    pub title: Option<String>,
    /// Soft-delete state, from the app's own `picasu:Trashed` marker. Stored
    /// in the sidecar so a database rebuild from the filesystem keeps trashed
    /// assets trashed; absent marker means not trashed.
    pub trashed: bool,
}

/// The app's own XMP namespace, for properties no standard namespace has a
/// key for. `Trashed` lives here: soft-delete is application state, not photo
/// metadata, and putting it in `dc:` or `xmp:` would claim a standard meaning.
pub const NS_PICASU: &str = "http://picasu.app/xmp/1.0/";

/// Prefix `NS_PICASU` is written with.
pub const NS_PICASU_PREFIX: &str = "picasu";

/// Register [`NS_PICASU`] with xmpkit's namespace registry.
///
/// xmpkit resolves a namespace by prefix or URI and refuses to write a
/// property in a namespace it does not know, and the registry is per thread,
/// so this runs before each write rather than once per process. Registering
/// the same pair twice is a no-op; a genuine failure (another component
/// owning the `picasu` prefix) leaves the write path reporting a
/// `BadSchema` error, which surfaces as an IO error to the caller.
pub fn ensure_picasu_namespace_registered() {
    if let Err(err) = xmpkit::register_namespace(NS_PICASU, NS_PICASU_PREFIX) {
        log::warn!("could not register the picasu XMP namespace: {err}");
    }
}

/// Extract XMP metadata from raw packet bytes (`.xmp` / `.albuminfo.xmp` content).
///
/// This is the seam for callers that already hold sidecar bytes (e.g.
/// `read_albuminfo`); callers with a file path use [`extract_xmp_data_from_file`].
///
/// The packet goes through [`XmpMeta::parse`] — sidecar packets never go
/// through `XmpFile::open` (xmpkit's packet-in-file scan cannot find them,
/// see the Iteration 0 spike). Invalid UTF-8 or a parse error yields empty
/// `XmpData`.
pub fn extract_xmp_data_from_packet(bytes: &[u8]) -> XmpData {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return XmpData::default();
    };
    XmpMeta::parse(text)
        .map(|meta| normalize(&meta))
        .unwrap_or_default()
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
///
/// Sidecar precedence: when a sidecar exists it is authoritative — its bytes
/// are read and parsed through [`XmpMeta::parse`], and *any* failure (unreadable
/// file, invalid UTF-8, parse error) yields empty `XmpData` without falling
/// back to the file's own XMP. Without a sidecar the embedded packet is read
/// via `XmpFile::open`; open errors, carriers xmpkit cannot handle, and files
/// without XMP all yield empty `XmpData`.
pub fn extract_xmp_data_from_file(path: &Path) -> XmpData {
    if let Some(sidecar) = discover_sidecar(path) {
        let Ok(text) = std::fs::read_to_string(&sidecar) else {
            return XmpData::default();
        };
        return XmpMeta::parse(&text)
            .map(|meta| normalize(&meta))
            .unwrap_or_default();
    }
    let mut file = XmpFile::new();
    match file.open(path) {
        Ok(()) => file.get_xmp().map(normalize).unwrap_or_default(),
        Err(_) => XmpData::default(),
    }
}

// ── Internals ─────────────────────────────────────────────────────────────────

/// Normalize xmpkit's values for the managed fields into `XmpData`.
///
/// Shapes observed with xmpkit 0.1.6: containers (`rdf:Bag`, `rdf:Alt`) arrive
/// as `XmpValue::Array([String])`; compact/attribute-form values arrive as a
/// plain `XmpValue::String` (subject comma-joined); `xmp:Rating` arrives as a
/// numeric `String`.
fn normalize(meta: &XmpMeta) -> XmpData {
    XmpData {
        tags: tags_from_subject(meta.get_property(ns::DC, "subject")),
        description: text_from_property(meta.get_property(ns::DC, "description")),
        rating: rating_from_property(meta.get_property(ns::XMP, "Rating")),
        title: text_from_property(meta.get_property(ns::DC, "title")),
        trashed: trashed_from_property(meta.get_property(NS_PICASU, "Trashed")),
    }
}

/// `dc:subject` → tags. `Array` items are flattened as-is; a plain `String`
/// (compact form) is split on commas, trimmed, empty parts dropped.
fn tags_from_subject(value: Option<XmpValue>) -> HashSet<String> {
    let mut tags = HashSet::new();
    match value {
        Some(XmpValue::Array(items)) => {
            for item in items {
                if let XmpValue::String(s) = item {
                    let s = s.trim();
                    if !s.is_empty() {
                        tags.insert(s.to_owned());
                    }
                }
            }
        }
        Some(XmpValue::String(s)) => {
            for part in s.split(',') {
                let part = part.trim();
                if !part.is_empty() {
                    tags.insert(part.to_owned());
                }
            }
        }
        _ => {}
    }
    tags
}

/// `dc:description` / `dc:title` → first element of an `Array` (document
/// order, e.g. the first `rdf:li` of an `rdf:Alt`) or the plain `String`.
/// Empty/absent → `None`.
fn text_from_property(value: Option<XmpValue>) -> Option<String> {
    let text = match value? {
        XmpValue::Array(items) => match items.into_iter().next()? {
            XmpValue::String(s) => s,
            _ => return None,
        },
        XmpValue::String(s) => s,
        _ => return None,
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// `xmp:Rating` → 0–5. The value parses as `i32` (to admit negatives such as
/// -1 = "rejected"), then goes through `u8::try_from` and the ≤ 5 filter —
/// the same clamp rule the byte-scan used (negative → None, 6 → None).
fn rating_from_property(value: Option<XmpValue>) -> Option<u8> {
    let XmpValue::String(raw) = value? else {
        return None;
    };
    raw.trim()
        .parse::<i32>()
        .ok()
        .and_then(|v| u8::try_from(v).ok())
        .filter(|&r| r <= 5)
}

/// `picasu:Trashed` → soft-delete state.
///
/// The app writes `XmpValue::Boolean`, but a packet written by hand or by a
/// future tool arrives as text, so `true`/`True`/`1` count as marked. Only a
/// marked packet reads as trashed: everything else — absent, `false`, a stray
/// string — reads as not trashed, because "not marked" is the state an
/// untouched asset must be in.
fn trashed_from_property(value: Option<XmpValue>) -> bool {
    match value {
        Some(XmpValue::Boolean(value)) => value,
        Some(XmpValue::String(raw)) => {
            let raw = raw.trim();
            raw.eq_ignore_ascii_case("true") || raw == "1"
        }
        Some(XmpValue::Integer(value)) => value != 0,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Fixtures ──────────────────────────────────────────────────────────────

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

    /// Compact/attribute-form packet: every managed field is an RDF attribute
    /// on `<rdf:Description>`, no child elements at all.
    const COMPACT_PACKET: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"
                 xmlns:xmp="http://ns.adobe.com/xap/1.0/"
                 dc:subject="c_one,c_two"
                 dc:description="Compact description"
                 dc:title="Compact title"
                 xmp:Rating="5"/>
</rdf:RDF>
</x:xmpmeta>"#;

    /// Malformed packet: `<dc:title>` is closed by `</dc:RDF>`, so the XML is
    /// not well-formed even though earlier sibling elements are intact.
    const MALFORMED_PACKET: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xmp="http://ns.adobe.com/xap/1.0/">
<dc:subject><rdf:Bag><rdf:li>leaked</rdf:li></rdf:Bag></dc:subject>
<dc:description><rdf:Alt><rdf:li xml:lang="x-default">Partial description</rdf:li></rdf:Alt></dc:description>
<xmp:Rating>4</xmp:Rating>
<dc:title><rdf:Alt><rdf:li xml:lang="x-default">Partial title</rdf:li></rdf:Alt></dc:RDF>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>"#;

    /// Hand-built JPEG: SOI + one APP1 XMP segment + EOI. No exiftool needed.
    fn jpeg_with_app1_xmp(packet: &str) -> Vec<u8> {
        const XMP_NS: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
        let mut payload = XMP_NS.to_vec();
        payload.extend_from_slice(packet.as_bytes());
        let len = u16::try_from(payload.len() + 2).expect("APP1 payload fits in one segment");
        let mut jpeg = vec![0xFF, 0xD8]; // SOI
        jpeg.extend_from_slice(&[0xFF, 0xE1]); // APP1
        jpeg.extend_from_slice(&len.to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(&[0xFF, 0xD9]); // EOI
        jpeg
    }

    fn assert_all_empty(data: &XmpData) {
        assert!(
            data.tags.is_empty(),
            "expected no tags, got {:?}",
            data.tags
        );
        assert_eq!(data.description, None);
        assert_eq!(data.rating, None);
        assert_eq!(data.title, None);
        assert!(!data.trashed, "expected not trashed");
    }

    /// A packet carrying the picasu-trash marker as attribute-form Boolean
    /// (`picasu:Trashed="true"`), beside a managed field so the packet is not
    /// otherwise empty.
    fn xmp_with_trashed_attribute(trashed: bool) -> String {
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:picasu="http://picasu.app/xmp/1.0/"
                  xmlns:dc="http://purl.org/dc/elements/1.1/"
                  picasu:Trashed="{trashed}">
 <dc:subject><rdf:Bag><rdf:li>sunset</rdf:li></rdf:Bag></dc:subject>
 </rdf:Description>
 </rdf:RDF>
 </x:xmpmeta>"#
        )
    }

    /// The element form (`<picasu:Trashed>true</picasu:Trashed>`), which a
    /// hand-written packet more likely carries.
    fn xmp_with_trashed_element(value: &str) -> String {
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:picasu="http://picasu.app/xmp/1.0/">
 <picasu:Trashed>{value}</picasu:Trashed>
 </rdf:Description>
 </rdf:RDF>
 </x:xmpmeta>"#
        )
    }

    // ── Managed-field extraction (ported intents) ─────────────────────────────

    #[test]
    fn extracts_all_fields() {
        let xmp = xmp_full(&["sunset", "travel"], "A beautiful sunset", 4);
        let data = extract_xmp_data_from_packet(xmp.as_bytes());
        assert_eq!(
            data.tags,
            HashSet::from(["sunset".to_string(), "travel".to_string()])
        );
        assert_eq!(data.description.as_deref(), Some("A beautiful sunset"));
        assert_eq!(data.rating, Some(4));
    }

    // ── Trash-state extraction ───────────────────────────────────────────────

    #[test]
    fn trashed_marker_reads_back_as_trashed() {
        let data = extract_xmp_data_from_packet(xmp_with_trashed_attribute(true).as_bytes());
        assert!(data.trashed);
        assert_eq!(
            data.tags,
            HashSet::from(["sunset".to_string()]),
            "reading the trash marker must not disturb the managed fields"
        );
    }

    #[test]
    fn untrashed_marker_reads_back_as_untrashed() {
        let data = extract_xmp_data_from_packet(xmp_with_trashed_attribute(false).as_bytes());
        assert!(!data.trashed);
    }

    /// The forms a hand- or externally-written packet takes: element text
    /// `true`, and the other spellings XMP booleans allow. Anything that is
    /// not one of them — including `false` and a non-boolean string — reads
    /// as not trashed, because "trashed" is the marked state.
    #[test]
    fn trashed_marker_tolerates_human_written_values() {
        for value in ["true", "True", "1"] {
            let data = extract_xmp_data_from_packet(xmp_with_trashed_element(value).as_bytes());
            assert!(data.trashed, "{value} must read as trashed");
        }
        for value in ["false", "False", "0", "yes", "banana"] {
            let data = extract_xmp_data_from_packet(xmp_with_trashed_element(value).as_bytes());
            assert!(!data.trashed, "{value} must read as not trashed");
        }
    }

    /// The marker's absence is the normal state, so an untouched packet (or a
    /// sidecar written before this property existed) reads as not trashed.
    #[test]
    fn absent_marker_reads_as_untrashed() {
        let data = extract_xmp_data_from_packet(xmp_full(&["sunset"], "desc", 3).as_bytes());
        assert!(!data.trashed);
    }

    #[test]
    fn rating_out_of_range_is_none() {
        let xmp = xmp_full(&[], "", 6);
        let data = extract_xmp_data_from_packet(xmp.as_bytes());
        assert_eq!(data.rating, None);
    }

    #[test]
    fn rating_negative_is_none() {
        // -1 = "rejected" in some tools; clamp rule: i32 → u8 → ≤ 5.
        let xmp = xmp_full(&[], "", -1);
        let data = extract_xmp_data_from_packet(xmp.as_bytes());
        assert_eq!(data.rating, None);
    }

    #[test]
    fn missing_fields_are_empty_or_none() {
        let xmp = xmp_packet_with_keywords(&["family"]);
        let data = extract_xmp_data_from_packet(xmp.as_bytes());
        assert_eq!(data.tags, HashSet::from(["family".to_string()]));
        assert_eq!(data.description, None);
        assert_eq!(data.rating, None);
    }

    #[test]
    fn extracts_keywords_from_dc_subject_bag() {
        let xmp = xmp_packet_with_keywords(&["family", "vacation"]);
        let data = extract_xmp_data_from_packet(xmp.as_bytes());
        assert_eq!(
            data.tags,
            HashSet::from(["family".to_string(), "vacation".to_string()])
        );
    }

    #[test]
    fn returns_empty_when_input_is_not_an_xmp_packet() {
        let data = extract_xmp_data_from_packet(b"\xff\xd8\xff plain jpeg, no xmp");
        assert_all_empty(&data);
    }

    #[test]
    fn returns_empty_from_file_when_carrier_has_no_xmp() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let plain = dir.path().join("photo.jpg");
        std::fs::write(&plain, b"\xff\xd8\xff plain jpeg, no xmp")
            .expect("failed to write fixture");
        let data = extract_xmp_data_from_file(&plain);
        assert_all_empty(&data);
    }

    #[test]
    fn returns_empty_set_when_dc_subject_is_absent_or_empty() {
        let xmp = xmp_packet_with_keywords(&[]);
        let data = extract_xmp_data_from_packet(xmp.as_bytes());
        assert!(data.tags.is_empty());
    }

    #[test]
    fn extracts_title() {
        let xmp = xmp_with_title("My Album");
        let data = extract_xmp_data_from_packet(xmp.as_bytes());
        assert_eq!(data.title.as_deref(), Some("My Album"));
    }

    #[test]
    fn title_is_none_when_absent() {
        let xmp = xmp_packet_with_keywords(&["family"]);
        let data = extract_xmp_data_from_packet(xmp.as_bytes());
        assert_eq!(data.title, None);
    }

    // ── New-behavior pins (xmpkit read) ───────────────────────────────────────

    #[test]
    fn compact_attribute_form_yields_managed_fields() {
        let data = extract_xmp_data_from_packet(COMPACT_PACKET.as_bytes());
        assert_eq!(
            data.tags,
            HashSet::from(["c_one".to_string(), "c_two".to_string()])
        );
        assert_eq!(data.description.as_deref(), Some("Compact description"));
        assert_eq!(data.title.as_deref(), Some("Compact title"));
        assert_eq!(data.rating, Some(5));
    }

    #[test]
    fn malformed_packet_yields_all_fields_empty() {
        let data = extract_xmp_data_from_packet(MALFORMED_PACKET.as_bytes());
        assert_all_empty(&data);
    }

    #[test]
    fn unreadable_packet_bytes_yield_empty() {
        // `read_albuminfo` hands raw file bytes to the packet seam; content
        // that is not valid UTF-8 must not produce partial data.
        let mut bytes = vec![0xFF, 0xFE, 0x00];
        bytes.extend_from_slice(
            b"<dc:subject><rdf:Bag><rdf:li>ghost</rdf:li></rdf:Bag></dc:subject>",
        );
        let data = extract_xmp_data_from_packet(&bytes);
        assert_all_empty(&data);
    }

    #[test]
    fn sidecar_wins_over_embedded_and_embedded_reads_when_sidecar_absent() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");

        let embedded = format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xmp="http://ns.adobe.com/xap/1.0/">
<dc:subject><rdf:Bag><rdf:li>emb_one</rdf:li></rdf:Bag></dc:subject>
<dc:description><rdf:Alt><rdf:li xml:lang="x-default">Embedded description</rdf:li></rdf:Alt></dc:description>
<dc:title><rdf:Alt><rdf:li xml:lang="x-default">Embedded title</rdf:li></rdf:Alt></dc:title>
<xmp:Rating>2</xmp:Rating>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>"#
        );
        std::fs::write(&photo, jpeg_with_app1_xmp(&embedded)).expect("failed to write jpeg");
        std::fs::write(&sidecar, COMPACT_PACKET).expect("failed to write sidecar");

        // Sidecar present → its values win over the embedded packet.
        let data = extract_xmp_data_from_file(&photo);
        assert_eq!(
            data.tags,
            HashSet::from(["c_one".to_string(), "c_two".to_string()])
        );
        assert_eq!(data.description.as_deref(), Some("Compact description"));
        assert_eq!(data.title.as_deref(), Some("Compact title"));
        assert_eq!(data.rating, Some(5));

        // Sidecar absent → the embedded packet is read.
        std::fs::remove_file(&sidecar).expect("failed to remove sidecar");
        let data = extract_xmp_data_from_file(&photo);
        assert_eq!(data.tags, HashSet::from(["emb_one".to_string()]));
        assert_eq!(data.description.as_deref(), Some("Embedded description"));
        assert_eq!(data.title.as_deref(), Some("Embedded title"));
        assert_eq!(data.rating, Some(2));
    }

    #[test]
    fn compressed_png_itxt_is_unsupported_and_yields_empty() {
        // Iteration 0 verdict: xmpkit refuses PNG iTXt with `compression_flag=1`
        // ("Compressed XMP in PNG not yet supported") → unclaimed, not a gap —
        // main's byte-scan never read real compressed content either. The seam
        // must return empty for such carriers, never partial data. The fixture
        // keeps the packet as plain bytes so the old byte-scan behavior
        // (scanning raw file bytes regardless of the flag) stays observable.
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let png_path = dir.path().join("photo.png");
        let packet = xmp_packet_with_keywords(&["hidden"]);
        std::fs::write(&png_path, png_with_compressed_itxt_xmp(&packet))
            .expect("failed to write fixture");
        let data = extract_xmp_data_from_file(&png_path);
        assert_all_empty(&data);
    }

    // ── PNG fixture builder (no image crates needed) ──────────────────────────

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// Minimal PNG carrying one XMP `iTXt` chunk with `compression_flag=1`
    /// (signature + IHDR + iTXt + IEND, valid CRCs).
    fn png_with_compressed_itxt_xmp(packet: &str) -> Vec<u8> {
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut crc_input = kind.to_vec();
            crc_input.extend_from_slice(data);
            out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        }
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        let mut itxt = b"XML:com.adobe.xmp\0".to_vec();
        itxt.push(1); // compression_flag = 1 (compressed)
        itxt.push(0); // compression method
        itxt.push(0); // empty language tag
        itxt.push(0); // empty translated keyword
        itxt.extend_from_slice(packet.as_bytes());
        chunk(&mut png, b"iTXt", &itxt);
        chunk(&mut png, b"IEND", &[]);
        png
    }
}
