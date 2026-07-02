use crate::model::abstract_data::AbstractData;
use crate::process::sanitize::is_valid_xml_char;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::io;
use std::path::Path;

/// Write (create or overwrite) the XMP sidecar for `abstract_data` with its
/// current managed metadata fields:
/// - `dc:subject` (tags), `dc:description`, `xmp:Rating` for all types
/// - `dc:title` additionally for albums, but only when explicitly
///   user-set (`metadata.custom_title`) — the directory-name-derived
///   default in `metadata.title` must never be baked into the sidecar, or
///   it would freeze and survive a later directory rename instead of being
///   re-derived from the new name.
///
/// Images/videos write `{basename}.{ext}.xmp` alongside their primary alias
/// file. Albums write `.albuminfo.xmp` inside `dir_path`. Items with no
/// aliases have nowhere on disk to write and are silently skipped (Ok
/// returned).
///
/// Uses an atomic temp-file + rename to avoid partial-write races.
/// Sidecar write failures are returned to the caller; callers should log and
/// treat them as non-fatal.
pub fn write_sidecar_for(abstract_data: &AbstractData) -> io::Result<()> {
    if let AbstractData::Album(album) = abstract_data {
        let sidecar = Path::new(&album.metadata.dir_path).join(".albuminfo.xmp");
        let content = format_xmp_packet(
            abstract_data.tag(),
            abstract_data.description(),
            abstract_data.rating(),
            album.metadata.custom_title.as_deref(),
        );
        return write_sidecar_content(&sidecar, &content);
    }

    let alias = abstract_data.alias();
    if alias.is_empty() {
        return Ok(());
    }
    let primary = Path::new(&alias[0].file);
    let sidecar = primary.with_extension("xmp");
    let content = format_xmp_packet(
        abstract_data.tag(),
        abstract_data.description(),
        abstract_data.rating(),
        None,
    );
    write_sidecar_content(&sidecar, &content)
}

fn write_sidecar_content(sidecar: &Path, content: &str) -> io::Result<()> {
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
    std::fs::rename(&tmp, sidecar)
}

fn format_xmp_packet(
    tags: &HashSet<String>,
    description: Option<&str>,
    rating: Option<u8>,
    title: Option<&str>,
) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
    out.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n");
    out.push_str("<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
    out.push_str("<rdf:Description rdf:about=\"\"\n");
    out.push_str("    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n");
    out.push_str("    xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\n");

    if let Some(t) = title.filter(|t| !t.is_empty()) {
        out.push_str("<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">");
        xml_escape_into(&mut out, t);
        out.push_str("</rdf:li></rdf:Alt></dc:title>\n");
    }

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
        let pkt = format_xmp_packet(&tags, Some("nice photo"), Some(3), None);
        assert!(pkt.contains("<rdf:li>cat</rdf:li>"));
        assert!(pkt.contains("<rdf:li>dog</rdf:li>"));
        assert!(pkt.contains("nice photo"));
        assert!(pkt.contains("<xmp:Rating>3</xmp:Rating>"));
    }

    #[test]
    fn packet_omits_empty_fields() {
        let pkt = format_xmp_packet(&HashSet::new(), None, None, None);
        assert!(!pkt.contains("dc:subject"));
        assert!(!pkt.contains("dc:description"));
        assert!(!pkt.contains("xmp:Rating"));
        assert!(!pkt.contains("dc:title"));
    }

    #[test]
    fn xml_special_chars_are_escaped() {
        let mut tags = HashSet::new();
        tags.insert("a&b".to_owned());
        let pkt = format_xmp_packet(&tags, Some("desc <with> \"quotes\""), None, None);
        assert!(pkt.contains("&amp;"));
        assert!(pkt.contains("&lt;"));
        assert!(pkt.contains("&quot;"));
    }

    #[test]
    fn packet_contains_title() {
        let pkt = format_xmp_packet(&HashSet::new(), None, None, Some("My Album"));
        assert!(pkt.contains("<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">My Album"));
    }

    mod album_sidecar {
        use super::*;
        use crate::model::album::{AlbumCombined, AlbumMetadata};
        use crate::model::object::{ObjectSchema, ObjectType};
        use arrayvec::ArrayString;

        fn make_dir_album(dir_path: String, custom_title: Option<String>) -> AbstractData {
            let id = ArrayString::from("alb").expect("failed to create test ArrayString");
            let mut object = ObjectSchema::new(id, ObjectType::Album);
            object.description = Some("A lovely trip".to_string());
            object.tags = HashSet::from(["vacation".to_string()]);
            object.rating = Some(4);
            AbstractData::Album(AlbumCombined {
                object,
                metadata: AlbumMetadata {
                    id,
                    // The auto-derived display title. Populated regardless of
                    // whether the user ever customized it — write_sidecar_for
                    // must key off `custom_title`, not this field.
                    title: Some("Vacation 2024".to_string()),
                    created_time: 0,
                    start_time: None,
                    end_time: None,
                    last_modified_time: 0,
                    cover: None,
                    item_count: 0,
                    item_size: 0,
                    share_list: Default::default(),
                    dir_path,
                    custom_title,
                },
            })
        }

        #[test]
        fn writes_albuminfo_xmp_for_dir_album() {
            let dir = tempfile::tempdir().expect("failed to create temp dir");
            let album = make_dir_album(
                dir.path().to_string_lossy().into_owned(),
                Some("Vacation 2024".to_string()),
            );

            write_sidecar_for(&album).expect("failed to write album sidecar");

            let sidecar_path = dir.path().join(".albuminfo.xmp");
            let content = std::fs::read_to_string(&sidecar_path).expect("sidecar not written");
            assert!(content.contains("Vacation 2024"));
            assert!(content.contains("A lovely trip"));
            assert!(content.contains("<rdf:li>vacation</rdf:li>"));
            assert!(content.contains("<xmp:Rating>4</xmp:Rating>"));
        }

        /// Regression test: editing a field other than title (rating/tags/
        /// description here) must not freeze the auto-derived display title
        /// into the sidecar, or a later directory rename would no longer be
        /// picked up.
        #[test]
        fn does_not_write_title_when_not_explicitly_customized() {
            let dir = tempfile::tempdir().expect("failed to create temp dir");
            let album = make_dir_album(dir.path().to_string_lossy().into_owned(), None);

            write_sidecar_for(&album).expect("failed to write album sidecar");

            let sidecar_path = dir.path().join(".albuminfo.xmp");
            let content = std::fs::read_to_string(&sidecar_path).expect("sidecar not written");
            assert!(!content.contains("dc:title"));
            assert!(!content.contains("Vacation 2024"));
        }
    }
}
