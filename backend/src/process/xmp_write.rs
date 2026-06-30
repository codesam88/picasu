use crate::model::abstract_data::AbstractData;
use crate::process::sanitize::is_valid_xml_char;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::io;
use std::path::Path;

/// Write (create or overwrite) the `.xmp` sidecar alongside the primary alias
/// file with the current managed metadata fields:
/// - `dc:subject` (tags)
/// - `dc:description`
/// - `xmp:Rating`
///
/// Uses an atomic temp-file + rename to avoid partial-write races.
/// Albums and items with no aliases are silently skipped (Ok returned).
/// Sidecar write failures are returned to the caller; callers should log and
/// treat them as non-fatal.
pub fn write_sidecar_for(abstract_data: &AbstractData) -> io::Result<()> {
    let alias = abstract_data.alias();
    if alias.is_empty() {
        return Ok(());
    }
    let primary = Path::new(&alias[0].file);
    write_sidecar_to(
        primary,
        abstract_data.tag(),
        abstract_data.description(),
        abstract_data.rating(),
    )
}

fn write_sidecar_to(
    original: &Path,
    tags: &HashSet<String>,
    description: Option<&str>,
    rating: Option<u8>,
) -> io::Result<()> {
    let sidecar = original.with_extension("xmp");
    let content = format_xmp_packet(tags, description, rating);
    let parent = sidecar.parent().unwrap_or(Path::new("."));
    let tmp_name = format!(
        ".{}.tmp",
        sidecar
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("xmp")
    );
    let tmp = parent.join(tmp_name);
    std::fs::write(&tmp, content.as_bytes())?;
    std::fs::rename(&tmp, &sidecar)
}

fn format_xmp_packet(
    tags: &HashSet<String>,
    description: Option<&str>,
    rating: Option<u8>,
) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
    out.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n");
    out.push_str("<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
    out.push_str("<rdf:Description rdf:about=\"\"\n");
    out.push_str("    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n");
    out.push_str("    xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\n");

    if !tags.is_empty() {
        let mut sorted: Vec<&String> = tags.iter().collect();
        sorted.sort();
        out.push_str("<dc:subject><rdf:Bag>\n");
        for tag in sorted {
            out.push_str("  <rdf:li>");
            xml_escape_into(&mut out, tag);
            out.push_str("</rdf:li>\n");
        }
        out.push_str("</rdf:Bag></dc:subject>\n");
    }

    if let Some(desc) = description.filter(|d| !d.is_empty()) {
        out.push_str("<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">");
        xml_escape_into(&mut out, desc);
        out.push_str("</rdf:li></rdf:Alt></dc:description>\n");
    }

    if let Some(r) = rating {
        let _ = writeln!(out, "<xmp:Rating>{r}</xmp:Rating>");
    }

    out.push_str("</rdf:Description>\n");
    out.push_str("</rdf:RDF>\n");
    out.push_str("</x:xmpmeta>\n");
    out.push_str("<?xpacket end=\"w\"?>\n");
    out
}

fn xml_escape_into(out: &mut String, s: &str) {
    for ch in s.chars() {
        if !is_valid_xml_char(ch) {
            continue; // drop characters forbidden by XML 1.0 §2.2
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_contains_all_fields() {
        let mut tags = HashSet::new();
        tags.insert("cat".to_owned());
        tags.insert("dog".to_owned());
        let pkt = format_xmp_packet(&tags, Some("nice photo"), Some(3));
        assert!(pkt.contains("<rdf:li>cat</rdf:li>"));
        assert!(pkt.contains("<rdf:li>dog</rdf:li>"));
        assert!(pkt.contains("nice photo"));
        assert!(pkt.contains("<xmp:Rating>3</xmp:Rating>"));
    }

    #[test]
    fn packet_omits_empty_fields() {
        let pkt = format_xmp_packet(&HashSet::new(), None, None);
        assert!(!pkt.contains("dc:subject"));
        assert!(!pkt.contains("dc:description"));
        assert!(!pkt.contains("xmp:Rating"));
    }

    #[test]
    fn xml_special_chars_are_escaped() {
        let mut tags = HashSet::new();
        tags.insert("a&b".to_owned());
        let pkt = format_xmp_packet(&tags, Some("desc <with> \"quotes\""), None);
        assert!(pkt.contains("&amp;"));
        assert!(pkt.contains("&lt;"));
        assert!(pkt.contains("&quot;"));
    }
}
