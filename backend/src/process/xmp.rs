use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Metadata extracted from a file's XMP packet or sidecar.
#[derive(Debug, Default)]
pub struct XmpData {
    pub tags: HashSet<String>,
    pub description: Option<String>,
    /// 0–5 per XMP `xmp:Rating`; -1 ("rejected") in some tools is clamped to None.
    pub rating: Option<u8>,
}

/// Extract XMP metadata from raw bytes (file contents or sidecar content).
///
/// Handles the three fields the app manages:
/// - `dc:subject`   → tags (`rdf:Bag` of `rdf:li`)
/// - `dc:description` → description (`rdf:Alt` of `rdf:li`)
/// - `xmp:Rating`   → rating (plain integer text node)
///
/// Limitations: only handles uncompressed, contiguous XMP packets.
/// Compact XMP (namespace shorthand, RDF attribute syntax) may not be matched.
pub fn extract_xmp_data(bytes: &[u8]) -> XmpData {
    XmpData {
        tags: extract_bag_field(bytes, b"<dc:subject>", b"</dc:subject>"),
        description: extract_alt_text(bytes, b"<dc:description>", b"</dc:description>"),
        // Parse as i32 to handle negative values (e.g. -1 = "rejected"); clamp to None.
        rating: extract_simple_integer(bytes, b"<xmp:Rating>", b"</xmp:Rating>")
            .and_then(|v| u8::try_from(v).ok().filter(|&r| r <= 5)),
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
}
