use crate::model::abstract_data::AbstractData;
use crate::process::exif::{XmpAssignment, XmpWriteOutcome, write_xmp_properties};
use crate::process::sanitize::is_valid_xml_char;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::io;
use std::path::Path;

/// The sidecar a dir-album's metadata lives in, inside the album's own
/// directory. Fixed rather than derived, so an album has exactly one name for
/// it and the read side (`process::dir_album`) looks in the same place.
const ALBUM_SIDECAR: &str = ".albuminfo.xmp";

/// What the app manages in one sidecar, and what it must leave alone.
///
/// The distinction is the contract: a managed field is *set* to the value the
/// app holds — including being removed, when the app holds nothing — while
/// everything else in the packet belongs to whatever else wrote it and is not
/// named here at all.
struct ManagedFields<'a> {
    /// `dc:subject`. Replaced whole, never appended to, so a tag removed in the
    /// app is gone from the sidecar after the next edit.
    tags: &'a HashSet<String>,
    /// `dc:description`, or removed when the app holds none.
    description: Option<&'a str>,
    /// `xmp:Rating`, or removed when the app holds none.
    rating: Option<u8>,
    /// `dc:title`, for albums only.
    title: TitleEdit<'a>,
}

/// What a write does with `dc:title`.
///
/// Three states, and the middle one is why this is not an `Option`: skipping a
/// property leaves whatever is there, which for a photo is right — the app never
/// manages a photo's title — and for an album would mean a title the user once
/// set survived the API call that cleared it. Albums are managed, so clearing
/// them is a removal; photos are not, so they are left alone.
enum TitleEdit<'a> {
    /// Not managed here; the property is not named in the write at all.
    Unmanaged,
    /// Managed: set to this value, or removed when it is empty.
    Set(&'a str),
}

/// Write (create or update) the XMP sidecar for `abstract_data` with its
/// current managed metadata fields:
/// - `dc:subject` (tags), `dc:description`, `xmp:Rating` for all types
/// - `dc:title` additionally for albums, but only when explicitly
///   user-set (`metadata.custom_title`) — the directory-name-derived
///   default in `metadata.title` must never be baked into the sidecar, or
///   it would freeze and survive a later directory rename instead of being
///   re-derived from the new name. An album with no custom title has the
///   property *removed*, so clearing a title really clears it.
///
/// Images/videos write `{basename}.ext.xmp` alongside their asset file.
/// Albums write `.albuminfo.xmp` inside `dir_path`. Items with no path
/// (pruned or album) have nowhere on disk to write and are silently skipped
/// (Ok returned).
///
/// # The file plus its sidecar is the source of truth
///
/// A sidecar is a shared file. It routinely carries properties this app either
/// does not model or does not own — a creator, a copyright notice, a rating from
/// a different tool, a location, an edit history, a namespace no reader here has
/// a rule for — and `furtherMetadata` exists to show the first kind to the user.
/// So an edit goes through `ExifTool` as a read-modify-write that names the
/// managed properties and nothing else: the properties it does not name come back
/// unchanged, which is the property-level guarantee
/// (`a_write_leaves_every_property_it_did_not_name_intact` is its engine-level
/// test). The price is that `ExifTool` re-serialises a packet it edits, so the
/// sidecar is not byte-stable across edits — only its properties are.
///
/// Sidecar write failures are returned to the caller; callers should log and
/// treat them as non-fatal.
pub fn write_sidecar_for(abstract_data: &AbstractData) -> io::Result<()> {
    if let AbstractData::Album(album) = abstract_data {
        let sidecar = Path::new(&album.metadata.dir_path).join(ALBUM_SIDECAR);
        return write_managed_fields(
            &sidecar,
            &ManagedFields {
                tags: abstract_data.tag(),
                description: abstract_data.description(),
                rating: abstract_data.rating(),
                title: TitleEdit::Set(album.metadata.custom_title.as_deref().unwrap_or_default()),
            },
        )
        .map(|_report| ());
    }

    let Some(file_entry) = abstract_data.path() else {
        return Ok(());
    };
    let sidecar = Path::new(&file_entry.file).with_extension("xmp");
    write_managed_fields(
        &sidecar,
        &ManagedFields {
            tags: abstract_data.tag(),
            description: abstract_data.description(),
            rating: abstract_data.rating(),
            // A photo's title is not a field the app edits, so it is neither
            // written nor cleared.
            title: TitleEdit::Unmanaged,
        },
    )
    .map(|_report| ())
}

/// The tag each managed property is written under.
///
/// `XMP:Rating` rather than `XMP-xmp:Rating`: both name the rating, and
/// `ExifTool` resolves the unqualified one. It reports the result back under the
/// namespace it resolved — `XMP-xmp:Rating` — which is what the read path
/// looks for, so the write spelling and the read key are deliberately not the
/// same string.
const TAG_SUBJECT: &str = "XMP-dc:Subject";
const TAG_DESCRIPTION: &str = "XMP-dc:Description";
const TAG_RATING: &str = "XMP:Rating";
const TAG_TITLE: &str = "XMP-dc:Title";

/// What happened to the packet that was in the sidecar before the write.
///
/// The distinction is the one an operator needs, and it is the payload of the log
/// line: [`write_sidecar_for`] returns only whether the write succeeded, so this
/// is where "the file was replaced, and here is why" is carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarWriteReport {
    /// The managed fields were written and the packet is the one that was there.
    Preserved,
    /// The packet `ExifTool` refused to write into has been replaced with a
    /// managed-only one. This is the only replacement this code can *recognise*,
    /// because it is the only one the engine reports as anything but a success —
    /// see the residual on the valid-XML case in the tests.
    ReplacedAfterRefusal,
}

impl SidecarWriteReport {
    /// The log line for a replacement, naming the file. At error level, once per
    /// replacement: whatever was lost was lost on the source of truth, and a
    /// deduplicated or rate-limited line would be scrolled away by the very log
    /// a user would be reading to notice.
    fn replacement_report(self, sidecar: &Path) -> String {
        match self {
            Self::Preserved => String::new(),
            Self::ReplacedAfterRefusal => format!(
                "XMP sidecar {} could not be parsed by ExifTool and is being replaced with a \
                 managed-only packet; properties it carried that ExifTool could not read are \
                 lost",
                sidecar.display()
            ),
        }
    }
}

/// Write the managed fields into `sidecar`, preserving everything else in it.
fn write_managed_fields(
    sidecar: &Path,
    fields: &ManagedFields<'_>,
) -> io::Result<SidecarWriteReport> {
    let assignments = fields.assignments();
    let assignments: Vec<XmpAssignment<'_>> = assignments
        .iter()
        .map(|(tag, value)| XmpAssignment {
            tag,
            value: value.as_deref(),
        })
        .collect();
    match write_xmp_properties(sidecar, &assignments) {
        Ok(XmpWriteOutcome::Updated | XmpWriteOutcome::Created) => {
            Ok(SidecarWriteReport::Preserved)
        }
        Ok(XmpWriteOutcome::Rejected) => {
            // The file is byte-identical to what it was, and what it was is a
            // packet ExifTool could not parse. Writing the managed fields over it
            // achieves what the successful path would have, and keeping it would
            // leave the user's acknowledged edit in a file no reader can use — so
            // the next reindex would revert it silently. The properties lost with
            // it were already unreadable.
            let report = SidecarWriteReport::ReplacedAfterRefusal;
            log::error!("{}", report.replacement_report(sidecar));
            overwrite_with_managed_packet(sidecar, fields).map(|()| report)
        }
        // Nothing is known about the file: the engine could not be reached, the
        // session would not start, the directory is not writable. The sidecar is
        // left exactly as it is — rewriting it here would destroy a good packet
        // over a missing dependency — and the caller is told, which is the only
        // place the failure becomes visible.
        Err(err) => Err(io::Error::other(err)),
    }
}

impl ManagedFields<'_> {
    /// The managed properties as owned `(tag, value)` pairs, one per list
    /// element.
    ///
    /// `ExifTool` replaces a list-valued property with its assignments and appends
    /// only on the `+=` spelling, so repeating the tag here is what makes the bag
    /// equal the managed set rather than the union of this edit and every edit
    /// before it. The values are owned rather than borrowed because each is the
    /// sanitised form of the stored one, not the stored one itself.
    fn assignments(&self) -> Vec<(&'static str, Option<String>)> {
        let mut sorted: Vec<&String> = self.tags.iter().collect();
        // Sorted so the argument list is the same for the same tag set, which
        // keeps a write reproducible and its log output stable.
        sorted.sort();

        let mut assignments = Vec::with_capacity(sorted.len() + 3);
        if sorted.is_empty() {
            // An empty set is a removal, not a skip. Skipping would leave the
            // previous bag in place, and the tags in it would come back on the
            // next index.
            assignments.push((TAG_SUBJECT, None));
        } else {
            for tag in sorted {
                assignments.push((TAG_SUBJECT, Some(single_line(tag))));
            }
        }
        assignments.push((TAG_DESCRIPTION, optional_text(self.description)));
        assignments.push((TAG_RATING, self.rating.map(|rating| rating.to_string())));
        match self.title {
            // Not named, so not touched: whatever is there stays.
            TitleEdit::Unmanaged => {}
            // Empty means the app holds no title for this album, which is a
            // removal — see `TitleEdit`.
            TitleEdit::Set(title) => assignments.push((TAG_TITLE, optional_text(non_empty(title)))),
        }
        assignments
    }
}

/// `value` sanitised when it has content in it, and `None` when it is absent or
/// empty.
///
/// An empty managed field is a removal rather than a blank, so that clearing a
/// description cannot leave behind a `dc:description` whose `x-default` entry is
/// the empty string — which reads back as a description the user never wrote.
fn optional_text(value: Option<&str>) -> Option<String> {
    value.filter(|text| !text.is_empty()).map(multiline)
}

/// `value`, or `None` when it is empty.
fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

/// Drop the characters XML 1.0 forbids, and keep a free-text value's line
/// structure.
///
/// The old serialiser dropped the same characters as it escaped, so this
/// carries that guarantee over to the value side: an invalid character reaches
/// the engine unescaped now, and the engine is not the thing that gets to
/// decide what is storable.
fn multiline(value: &str) -> String {
    value
        .chars()
        .filter(|&c| is_valid_xml_char(c) && c != '\r')
        .collect()
}

/// A tag is a single line by contract (`sanitize_tag`), and one that is not
/// could not be written at all: the argument file is line-oriented, so a
/// newline in a value is unrepresentable in the argument the tag is carried in.
fn single_line(value: &str) -> String {
    value
        .chars()
        .filter(|&c| is_valid_xml_char(c) && c != '\n' && c != '\r')
        .collect()
}

/// Replace `sidecar` with a managed-only packet, atomically.
///
/// The controlled writer, kept for the one case the engine cannot handle: a
/// sidecar whose XMP `ExifTool` refuses to parse. `ExifTool` re-serialises what
/// it can read, so there is nothing for it to re-serialise here, and a packet
/// this writes is the only shape that is guaranteed to parse whatever the
/// original was.
///
/// Temp-file + rename, so a failure part-way leaves the previous sidecar whole.
/// That is the same atomicity `ExifTool` provides for its own writes — it stages
/// the new packet beside the target and renames it over only on success, measured
/// by `a_write_reports_whether_it_updated_created_or_was_refused` — and it is
/// the reason the write path does not have to reimplement the engine's.
fn overwrite_with_managed_packet(sidecar: &Path, fields: &ManagedFields<'_>) -> io::Result<()> {
    let content = format_xmp_packet(
        fields.tags,
        fields.description,
        fields.rating,
        match fields.title {
            TitleEdit::Unmanaged => None,
            TitleEdit::Set(title) => non_empty(title),
        },
    );
    write_sidecar_content(sidecar, &content)
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
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use crate::process::xmp::{NativeMetadata, read_xmp_packet};
    use arrayvec::ArrayString;
    use std::collections::BTreeMap;

    /// An image record at `file` with the managed fields a test wants to write.
    fn photo_with(
        dir: &Path,
        file: &str,
        tags: &[&str],
        description: Option<&str>,
        rating: Option<u8>,
    ) -> AbstractData {
        let mut metadata = ImageMetadata::new(0, 4, 3, "jpg".to_string());
        metadata.path = Some(crate::model::response::FileEntry::new(&dir.join(file), 0));
        let mut object = ObjectSchema::new(
            ArrayString::from("img").expect("hash fits"),
            ObjectType::Image,
        );
        object.tags = tags.iter().map(|t| (*t).to_string()).collect();
        object.description = description.map(str::to_string);
        object.rating = rating;
        AbstractData::Image(ImageCombined { object, metadata })
    }

    /// A sidecar carrying managed properties in the app's own namespaces and,
    /// beside them, properties from namespaces the app does not model.
    ///
    /// The unmanaged set is what the preservation test is about: `photoshop:*`
    /// and `tiff:Make` are what another tool wrote, `Iptc4xmpCore:*` is a
    /// namespace the app has no read rule for, and `xmpRights:Marked` sits in
    /// the *same* XMP namespace the app writes a managed property into — a
    /// writer that replaced a whole namespace instead of the named properties
    /// in it would pass on the others and fail on that one.
    const SEEDED_SIDECAR: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about=""
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:xmpRights="http://ns.adobe.com/xap/1.0/rights/"
    xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
    xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
    xmlns:Iptc4xmpCore="http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/">
  <dc:subject><rdf:Bag><rdf:li>stale-tag</rdf:li><rdf:li>also-stale</rdf:li></rdf:Bag></dc:subject>
  <dc:description><rdf:Alt><rdf:li xml:lang="x-default">stale description</rdf:li></rdf:Alt></dc:description>
  <xmp:Rating>1</xmp:Rating>
  <xmpRights:Marked>True</xmpRights:Marked>
  <photoshop:City>Zürich</photoshop:City>
  <photoshop:Country>Switzerland</photoshop:Country>
  <tiff:Make>SeedCam</tiff:Make>
  <Iptc4xmpCore:Location>Alps</Iptc4xmpCore:Location>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
"#;

    /// The unmanaged properties in [`SEEDED_SIDECAR`], and what each is.
    ///
    /// The keys are the ones the *reader* reports, which is not the spelling the
    /// packet uses: ExifTool resolves `Iptc4xmpCore` to the family-1 name
    /// `iptcCore` and reports it as `XMP-iptcCore:Location`. Asserting the
    /// packet's own spelling would fail on a preservation that had in fact
    /// happened. The one value that is not verbatim is `xmpRights:Marked`: it is
    /// an XMP boolean, and ExifTool prints booleans in its own lower-case form.
    const UNMANAGED: &[(&str, &str)] = &[
        ("photoshop:City", "Zürich"),
        ("photoshop:Country", "Switzerland"),
        ("tiff:Make", "SeedCam"),
        ("iptcCore:Location", "Alps"),
        ("xmpRights:Marked", "true"),
    ];

    /// The XMP properties of the sidecar at `path`, read back through the
    /// production reader, keyed `dc:subject` / `xmp:Rating` / … as
    /// `XMP-<ns>:<Tag>`.
    fn properties_of(path: &Path) -> BTreeMap<String, String> {
        let record = crate::process::exif::read_metadata_record(path)
            .unwrap_or_else(|err| panic!("the written sidecar must be readable: {err:#}"));
        let serde_json::Value::Object(entries) = &record else {
            panic!("a record is an object, as the reader returns one")
        };
        entries
            .iter()
            .filter(|(key, _)| key.starts_with("XMP-"))
            .filter_map(|(key, value)| {
                Some((
                    key.clone(),
                    crate::process::exif::json_value_to_string(value)?,
                ))
            })
            .collect()
    }

    /// The natively modelled fields of the sidecar at `path`, read through the
    /// production read path.
    ///
    /// [`read_xmp_packet`] answers a failed read with default fields, which in a
    /// test that just wrote a file is indistinguishable from a write that put
    /// nothing there — an empty tag set and an unreadable file look the same.
    /// The record is read once here for its own sake so that a read which fails
    /// is reported as the engine error it is, and then the real read path is
    /// asked for the mapping.
    fn read_sidecar(path: &Path) -> NativeMetadata {
        crate::process::exif::read_metadata_record(path)
            .unwrap_or_else(|err| panic!("the written sidecar must be readable: {err:#}"));
        read_xmp_packet(path)
    }

    /// The whole `element` in the sidecar at `path`, as raw markup, so the
    /// assertion does not depend on how the packet was serialised. `None` when
    /// the packet carries no such element at all.
    ///
    /// Raw markup rather than the read-back value because whitespace inside an
    /// element is exactly what some of these tests are about, and the reader
    /// trims it.
    fn element_of(path: &Path, element: &str) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        let start = text.find(&format!("<{element}"))?;
        let rest = &text[start..];
        // These elements nest (`dc:subject` holds `rdf:Bag` and `rdf:li`) but
        // never contain another element of their own name, so the first closing
        // tag of the same name ends it.
        let end = rest.find(&format!("</{element}>"))?;
        Some(rest[..end + element.len() + 3].to_string())
    }

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

    /// Editing a managed field must not delete XMP the app does not model.
    ///
    /// This is the whole point of the read-modify-write path. The file plus its
    /// sidecar is the source of truth, and `furtherMetadata` exists to surface
    /// exactly the properties a write must not destroy — a packet rewritten from
    /// the app's four managed fields takes them with it, silently, on the first
    /// tag edit a user makes.
    #[test]
    fn editing_a_managed_field_preserves_unmanaged_sidecar_properties() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        std::fs::write(&sidecar, SEEDED_SIDECAR).expect("seed the sidecar");
        let before = properties_of(&sidecar);

        // One managed field changed; nothing else about the record did.
        write_sidecar_for(&photo_with(
            dir.path(),
            "photo.jpg",
            &["fresh-tag"],
            Some("stale description"),
            Some(1),
        ))
        .expect("the sidecar write should succeed");

        let after = properties_of(&sidecar);
        assert_eq!(
            after.get("XMP-dc:Subject"),
            Some(&"fresh-tag".to_string()),
            "the bag must be exactly the managed tag set"
        );
        for (element, value) in UNMANAGED {
            assert_eq!(
                after.get(&format!("XMP-{element}")),
                before.get(&format!("XMP-{element}")),
                "{element} is unmanaged and must survive the edit unchanged"
            );
            assert_eq!(
                after.get(&format!("XMP-{element}")).map(String::as_str),
                Some(*value),
                "the seed carries {element}, or this assertion is vacuous"
            );
        }
    }

    /// The preservation is not a one-off: a second edit must not lose what the
    /// first one left, and a property added by another tool between two edits
    /// must still be there after the second.
    #[test]
    fn repeated_edits_keep_preserving() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        std::fs::write(&sidecar, SEEDED_SIDECAR).expect("seed the sidecar");

        write_sidecar_for(&photo_with(
            dir.path(),
            "photo.jpg",
            &["first"],
            Some("first description"),
            Some(2),
        ))
        .expect("first write");
        // Another tool adds a property the app has no rule for.
        let after_first = std::fs::read_to_string(&sidecar).expect("read back");
        let added = after_first.replace(
            "<xmpRights:Marked>True</xmpRights:Marked>",
            "<xmpRights:Marked>True</xmpRights:Marked>\n  <photoshop:State>Bern</photoshop:State>",
        );
        assert_ne!(
            added, after_first,
            "the seed must contain the marked property"
        );
        std::fs::write(&sidecar, added).expect("add the second tool's property");

        write_sidecar_for(&photo_with(
            dir.path(),
            "photo.jpg",
            &["second"],
            Some("second description"),
            Some(5),
        ))
        .expect("second write");

        let after = properties_of(&sidecar);
        assert_eq!(
            after.get("XMP-dc:Subject"),
            Some(&"second".to_string()),
            "the bag must be the new managed set, not the union of both"
        );
        assert_eq!(
            after.get("XMP-photoshop:State"),
            Some(&"Bern".to_string()),
            "a property added between two edits must survive the second"
        );
        for (element, _) in UNMANAGED {
            assert!(
                after.contains_key(&format!("XMP-{element}")),
                "{element} must still be there after a second edit"
            );
        }
    }

    /// A tag removed from the managed set is gone from the sidecar, however
    /// many edits came before.
    ///
    /// `ExifTool` replaces a list-valued property with its assignments and appends
    /// only on the `+=` spelling, so this is the test that says which one is in
    /// use: on `+=` the seed's two tags survive beside the new one and the bag
    /// grows on every edit.
    #[test]
    fn a_removed_tag_does_not_survive_the_next_edit() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        std::fs::write(&sidecar, SEEDED_SIDECAR).expect("seed the sidecar");

        for round in ["one", "two", "three"] {
            write_sidecar_for(&photo_with(dir.path(), "photo.jpg", &[round], None, None))
                .expect("write");
            assert_eq!(
                properties_of(&sidecar).get("XMP-dc:Subject"),
                Some(&round.to_string()),
                "round {round}: the bag must equal the managed tag set exactly"
            );
        }
        let bag = element_of(&sidecar, "dc:subject").expect("a bag was written");
        assert_eq!(
            bag.matches("<rdf:li>").count(),
            1,
            "the seed's stale tags must not have accumulated: {bag}"
        );
    }

    /// What was written is what the app reads back.
    ///
    /// The read path is the other end of the same contract, so this goes through
    /// [`crate::process::xmp::read_xmp_packet`] — the function a dir-album's
    /// `.albuminfo.xmp` is hydrated by — rather than through the engine directly,
    /// so a packet that parses but maps to the wrong fields fails here.
    #[test]
    fn managed_fields_round_trip_through_the_read_path() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        let description = "first line\nsecond line with \"quotes\" & <angle>";

        write_sidecar_for(&photo_with(
            dir.path(),
            "photo.jpg",
            &["alpha", "beta"],
            Some(description),
            Some(4),
        ))
        .expect("write");

        let read = read_sidecar(&sidecar);
        assert_eq!(
            read.tags,
            HashSet::from(["alpha".to_string(), "beta".to_string()]),
            "tags must come back as the managed set"
        );
        assert_eq!(
            read.description.as_deref(),
            Some(description),
            "a description with markup and newlines must come back whole"
        );
        assert_eq!(read.rating, Some(4));
    }

    /// Hostile values are user input, and the two ends of the path have to agree
    /// on them: a tag that looks like an ExifTool argument, one that starts with
    /// the assignment character, and non-ASCII text.
    #[test]
    fn awkward_managed_values_round_trip() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        let awkward = [
            "-XMP-dc:Subject=injected",
            "a=b",
            "=leading-equals",
            "quote'and\"quote",
            "amp&and<lt>and>gt",
            "Zürich ✓ 日本語",
        ];

        write_sidecar_for(&photo_with(
            dir.path(),
            "photo.jpg",
            &awkward,
            Some(""),
            Some(0),
        ))
        .expect("write");

        let read = read_sidecar(&sidecar);
        assert_eq!(
            read.tags,
            awkward.iter().map(|t| (*t).to_string()).collect(),
            "every tag must survive the round trip, none merged and none dropped"
        );
        assert_eq!(
            read.description, None,
            "an empty description is an absent one, not a blank"
        );
        assert_eq!(read.rating, Some(0), "0 is a rating, not an absent one");
    }

    /// A tag with surrounding whitespace, and where each end of it is lost.
    ///
    /// Split in two so neither side is blamed for the other. The packet keeps
    /// the *trailing* blank and loses the *leading* one, because ExifTool's
    /// argument parser consumes a single blank after the `=` of `-TAG=value` —
    /// measured, and the reason a tag is not a value this path can round-trip
    /// byte for byte. [`crate::process::xmp::read_xmp_packet`] then trims both
    /// ends, because the keyword mapper normalises every value it takes
    /// (`process::xmp`, `keyword_of`).
    ///
    /// Both normalisations are read-side or transport decisions and not this
    /// change's to move, and neither loses anything observable: the app only
    /// ever sees the trimmed form, so a tag stored as `" padded "` and one stored
    /// as `"padded"` are the same tag to every reader. Pinning the boundary here
    /// says the write is not what drops the characters, so the follow-up on read
    /// semantics starts from a known position.
    #[test]
    fn a_tag_with_surrounding_whitespace_keeps_its_trailing_blank_in_the_packet() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");

        write_sidecar_for(&photo_with(
            dir.path(),
            "photo.jpg",
            &[" padded "],
            None,
            None,
        ))
        .expect("write");

        let bag = element_of(&sidecar, "dc:subject").expect("a bag was written");
        assert!(
            bag.contains("<rdf:li>padded </rdf:li>"),
            "the sidecar must keep the trailing blank, found: {bag}"
        );
        assert!(
            !bag.contains("<rdf:li> padded"),
            "the leading blank is consumed by the engine's argument parsing, found: {bag}"
        );
        assert_eq!(
            read_sidecar(&sidecar).tags,
            HashSet::from(["padded".to_string()]),
            "and the reader trims what is left"
        );
    }

    /// An existing sidecar whose XMP `ExifTool` refuses to parse.
    ///
    /// The measured position, and the one pinned here: a packet whose envelope
    /// `ExifTool` recognises but whose XML does not parse is *refused* — the file
    /// comes back byte-identical, and the write falls back to the controlled
    /// managed-only packet. The properties lost with it were already unreadable
    /// to every reader, this one included, and keeping the file would leave the
    /// user's acknowledged edit in a packet nothing can read, so the next reindex
    /// would revert it silently.
    ///
    /// The alternative — leave the file alone and report the write as failed — is
    /// `exif.rs`'s `Rejected` outcome taken as an error rather than as a
    /// decision. It is rejected here for the reason above: the cache would then
    /// hold an edit that is in no file at all. Which of the two an operator would
    /// rather have is a product question this test does not settle.
    #[test]
    fn a_sidecar_exiftool_refuses_is_replaced_by_a_clean_managed_packet() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        // A packet that is not closed, so there is nothing to write into.
        let broken = SEEDED_SIDECAR
            .replace("</rdf:Bag></dc:subject>", "")
            .replace("</rdf:Alt></dc:description>", "");
        assert_ne!(broken, SEEDED_SIDECAR, "the damage must be applied");
        std::fs::write(&sidecar, &broken).expect("write the broken packet");

        let report = write_managed_fields(
            &sidecar,
            &ManagedFields {
                tags: &HashSet::from(["recovered".to_string()]),
                description: Some("recovered description"),
                rating: Some(3),
                title: TitleEdit::Unmanaged,
            },
        )
        .expect("a refused write still has to leave the edit applied");

        assert_eq!(
            report,
            SidecarWriteReport::ReplacedAfterRefusal,
            "the replacement has to be reported, not just done"
        );
        let read = read_sidecar(&sidecar);
        assert_eq!(
            read.tags,
            HashSet::from(["recovered".to_string()]),
            "the managed set must be applied"
        );
        assert_eq!(read.description.as_deref(), Some("recovered description"));
        assert_eq!(read.rating, Some(3));
    }

    /// A packet cut off part-way: the other measured spelling of "unreadable",
    /// and the one an interrupted copy or a full disk produces.
    ///
    /// Same outcome as the structurally broken packet and for the same reason —
    /// `ExifTool` finds the envelope, cannot parse what is inside, and refuses —
    /// which is the point of pinning both: the two must not behave differently
    /// from each other.
    #[test]
    fn a_truncated_sidecar_is_replaced_by_a_readable_packet() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        // The head of a real packet, cut before the closing tags.
        let truncated: String = SEEDED_SIDECAR.chars().take(400).collect();
        std::fs::write(&sidecar, &truncated).expect("write the truncated packet");

        let report = write_managed_fields(
            &sidecar,
            &ManagedFields {
                tags: &HashSet::from(["kept".to_string()]),
                description: Some("after truncation"),
                rating: Some(4),
                title: TitleEdit::Unmanaged,
            },
        )
        .expect("the write should succeed");

        assert_eq!(report, SidecarWriteReport::ReplacedAfterRefusal);
        let read = read_sidecar(&sidecar);
        assert_eq!(read.tags, HashSet::from(["kept".to_string()]));
        assert_eq!(read.description.as_deref(), Some("after truncation"));
        assert_eq!(read.rating, Some(4));
    }

    /// A photo's `dc:title` belongs to whoever wrote it, and an edit must leave
    /// it there.
    ///
    /// This is the one managed property the app does not manage on a photo, and
    /// it is a deliberate asymmetry with an album rather than an oversight: the
    /// app never writes a photo's title, so naming the property at all would
    /// delete a title another tool put there — the exact loss this path exists to
    /// prevent. An album's title *is* managed, which is why clearing it there
    /// removes the property (`clearing_a_custom_title_removes_it_from_the_sidecar`)
    /// and the two cannot share one code path.
    #[test]
    fn a_photos_title_is_left_where_someone_else_put_it() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        // A packet carrying a title alongside the managed fields.
        let seeded = SEEDED_SIDECAR.replace(
            "<xmp:Rating>1</xmp:Rating>",
            "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Somebody Else's Title</rdf:li>\
             </rdf:Alt></dc:title>\n  <xmp:Rating>1</xmp:Rating>",
        );
        assert!(
            seeded.contains("Somebody Else"),
            "the seed must carry a title"
        );
        std::fs::write(&sidecar, seeded).expect("seed the sidecar");

        write_sidecar_for(&photo_with(
            dir.path(),
            "photo.jpg",
            &["edited"],
            Some("a description"),
            Some(3),
        ))
        .expect("write");

        let after = properties_of(&sidecar);
        assert_eq!(
            after.get("XMP-dc:Title"),
            Some(&"Somebody Else's Title".to_string()),
            "a photo's title is not the app's field to change"
        );
        assert_eq!(
            after.get("XMP-dc:Subject"),
            Some(&"edited".to_string()),
            "the managed fields are still written"
        );
    }

    /// A sidecar of bytes `ExifTool` cannot read at all.
    ///
    /// Measured as a *refusal* — the file comes back byte-identical — so the
    /// controlled managed-only packet replaces it and the replacement is
    /// reported. This is the shape an interrupted copy, a full disk, or a
    /// directory index dumped into the file produces.
    #[test]
    fn a_sidecar_of_bytes_that_are_not_xmp_is_replaced_after_a_refusal() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        std::fs::write(&sidecar, b"\x00\x01\x02 not xmp at all \xff").expect("write junk");

        let report = write_managed_fields(
            &sidecar,
            &ManagedFields {
                tags: &HashSet::from(["only".to_string()]),
                description: Some("description"),
                rating: Some(2),
                title: TitleEdit::Unmanaged,
            },
        )
        .expect("the write should succeed");

        assert_eq!(
            report,
            SidecarWriteReport::ReplacedAfterRefusal,
            "a replacement of a whole file has to be reported, not just done"
        );
        let read = read_sidecar(&sidecar);
        assert_eq!(read.tags, HashSet::from(["only".to_string()]));
        assert_eq!(read.description.as_deref(), Some("description"));
        assert_eq!(read.rating, Some(2));
    }

    /// A sidecar that is well-formed XML but not XMP — the one lossy case this
    /// code does *not* detect, pinned so the gap is visible rather than assumed
    /// away.
    ///
    /// Measured: `ExifTool` reports this as an ordinary successful write
    /// (`1 image files updated`, nothing on stderr) after replacing the document
    /// with a fresh packet. There is no signal to tell it apart from editing a
    /// real packet, so the report says `Preserved` and nothing is logged, even
    /// though the previous bytes are gone. The outcome is still the desired one
    /// — a readable packet holding the managed set — but the operator is not
    /// told, and this code does not claim to tell them.
    ///
    /// The two ways that could change, neither taken here: ExifTool could report
    /// the case distinctly, or this code could pre-read the file — which means
    /// re-introducing a byte-level XMP scan, which
    /// `.plan/exiftool-metadata-engine.md` decision 5 retired. The first is
    /// upstream's to fix; the second is the trade the plan made deliberately.
    #[test]
    fn a_sidecar_of_xml_that_is_not_xmp_is_replaced_without_being_detected() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        let not_xmp = b"<?xml version=\"1.0\"?><library><shelf>books</shelf></library>";
        std::fs::write(&sidecar, not_xmp).expect("write the document");

        assert_eq!(
            write_managed_fields(
                &sidecar,
                &ManagedFields {
                    tags: &HashSet::from(["only".to_string()]),
                    description: Some("description"),
                    rating: Some(2),
                    title: TitleEdit::Unmanaged,
                },
            )
            .expect("the write should succeed"),
            SidecarWriteReport::Preserved,
            "there is no signal to distinguish this from editing a real packet"
        );
        let read = read_sidecar(&sidecar);
        assert_eq!(
            read.tags,
            HashSet::from(["only".to_string()]),
            "the managed set is still applied, which is the part that matters"
        );
        let text = std::fs::read_to_string(&sidecar).expect("read back");
        assert!(
            !text.contains("books"),
            "and the non-XMP document is gone: {text}"
        );
    }

    /// A sidecar that did not exist, and an empty one: the two cases where
    /// `ExifTool` creating a packet is the whole point and nothing is lost.
    #[test]
    fn creating_a_sidecar_reports_nothing_lost() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let tags = HashSet::from(["new".to_string()]);
        let fields = || ManagedFields {
            tags: &tags,
            description: Some("described"),
            rating: Some(1),
            title: TitleEdit::Unmanaged,
        };

        let absent = dir.path().join("absent.xmp");
        assert_eq!(
            write_managed_fields(&absent, &fields()).expect("write a new sidecar"),
            SidecarWriteReport::Preserved
        );

        let empty = dir.path().join("empty.xmp");
        std::fs::write(&empty, b"").expect("write an empty file");
        assert_eq!(
            write_managed_fields(&empty, &fields()).expect("write into an empty file"),
            SidecarWriteReport::Preserved,
            "an empty file had nothing in it to lose"
        );
    }

    /// A sidecar the app wrote earlier, and a readable one another tool wrote:
    /// neither is reported, because nothing was lost.
    #[test]
    fn writing_over_a_readable_packet_reports_nothing_lost() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        let tags = HashSet::from(["edited".to_string()]);
        let fields = || ManagedFields {
            tags: &tags,
            description: Some("edited description"),
            rating: Some(5),
            title: TitleEdit::Unmanaged,
        };
        // A packet this same writer produced.
        write_managed_fields(&sidecar, &fields()).expect("first write");
        assert_eq!(
            write_managed_fields(&sidecar, &fields()).expect("second write"),
            SidecarWriteReport::Preserved
        );
        // A packet another tool produced, with properties in it.
        std::fs::write(&sidecar, SEEDED_SIDECAR).expect("seed another tool's packet");
        assert_eq!(
            write_managed_fields(&sidecar, &fields()).expect("edit the other tool's packet"),
            SidecarWriteReport::Preserved
        );
    }

    /// A write that cannot reach the engine leaves the previous sidecar as it
    /// was.
    ///
    /// The distinction that matters: a *file* `ExifTool` refuses is replaced
    /// (above), while an engine that is not there at all is not the file's
    /// fault, and rewriting the sidecar behind the user's back would destroy a
    /// perfectly good packet because a dependency is missing. The directory is
    /// made unwritable to stop the write at the filesystem, which is also the
    /// shape of a real failure: a library on a read-only mount, or a sidecar in a
    /// directory the process may not write.
    #[cfg(unix)]
    #[test]
    fn an_unwritable_sidecar_is_left_untouched() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let sidecar = dir.path().join("photo.xmp");
        std::fs::write(&sidecar, SEEDED_SIDECAR).expect("seed the sidecar");
        let seeded = std::fs::read(&sidecar).expect("read the seed back");

        // Read and execute, no write: nothing can be created or replaced here.
        let permissions = std::fs::Permissions::from_mode(0o500);
        std::fs::set_permissions(dir.path(), permissions).expect("make the directory read-only");
        let outcome = write_sidecar_for(&photo_with(
            dir.path(),
            "photo.jpg",
            &["new-tag"],
            Some("new description"),
            Some(5),
        ));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("restore the directory");

        assert!(
            outcome.is_err(),
            "a write that cannot land must be reported, not swallowed"
        );
        assert_eq!(
            std::fs::read(&sidecar).expect("the sidecar is still there"),
            seeded,
            "a failed write must leave the previous sidecar byte-identical"
        );
    }

    /// A successful write leaves nothing of its own behind.
    ///
    /// The write stages a value file for any value an argument cannot carry, so
    /// this is what stops a value file from outliving the write and being found
    /// by the next index as if it were a sidecar.
    #[test]
    fn a_write_leaves_no_temporary_files_behind() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        write_sidecar_for(&photo_with(
            dir.path(),
            "photo.jpg",
            &["tag"],
            Some("a description\nover two lines"),
            Some(1),
        ))
        .expect("write");

        let leftovers: Vec<String> = std::fs::read_dir(dir.path())
            .expect("read the directory")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "photo.jpg" && name != "photo.xmp")
            .collect();
        assert!(
            leftovers.is_empty(),
            "the write left files beside the sidecar: {leftovers:?}"
        );
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
                    is_trashed: false,
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
            let read = read_sidecar(&sidecar_path);
            assert_eq!(read.tags, HashSet::from(["vacation".to_string()]));
            assert_eq!(read.description.as_deref(), Some("A lovely trip"));
            assert_eq!(read.title.as_deref(), Some("Vacation 2024"));
            assert_eq!(read.rating, Some(4));
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
            let read = read_sidecar(&sidecar_path);
            assert_eq!(
                read.title, None,
                "the directory-derived default must not reach the sidecar"
            );
            let content = std::fs::read_to_string(&sidecar_path).expect("sidecar not written");
            assert!(!content.contains("Vacation 2024"));
        }

        /// An album's sidecar is preserved the same way a photo's is, and its
        /// managed set is one property wider: the title is managed too, so an
        /// edit of any other field must not drop a title a user set.
        #[test]
        fn an_album_edit_preserves_unmanaged_properties_and_its_title() {
            let dir = tempfile::tempdir().expect("failed to create temp dir");
            let sidecar = dir.path().join(".albuminfo.xmp");
            std::fs::write(&sidecar, SEEDED_SIDECAR).expect("seed the sidecar");

            write_sidecar_for(&make_dir_album(
                dir.path().to_string_lossy().into_owned(),
                Some("Chosen Name".to_string()),
            ))
            .expect("write");

            let after = properties_of(&sidecar);
            assert_eq!(
                after.get("XMP-dc:Title"),
                Some(&"Chosen Name".to_string()),
                "the customised title is a managed field and must be written"
            );
            assert_eq!(after.get("XMP-xmp:Rating"), Some(&"4".to_string()));
            for (element, value) in UNMANAGED {
                assert_eq!(
                    after.get(&format!("XMP-{element}")).map(String::as_str),
                    Some(*value),
                    "{element} is unmanaged and must survive an album edit"
                );
            }
        }

        /// Clearing a custom title has to remove the property from the sidecar,
        /// not merely stop writing it.
        ///
        /// This is the one case where the read-modify-write path could silently
        /// regress: an edit that does not mention a property leaves it alone, so a
        /// title the user had set would survive `PUT /put/set_album_title` with
        /// `title: null` and be re-hydrated into the album on the next read. The
        /// title is therefore *cleared* rather than skipped when the album has no
        /// custom title, and this is what holds that.
        #[test]
        fn clearing_a_custom_title_removes_it_from_the_sidecar() {
            let dir = tempfile::tempdir().expect("failed to create temp dir");
            let sidecar = dir.path().join(".albuminfo.xmp");

            write_sidecar_for(&make_dir_album(
                dir.path().to_string_lossy().into_owned(),
                Some("Chosen Name".to_string()),
            ))
            .expect("first write");
            assert!(properties_of(&sidecar).contains_key("XMP-dc:Title"));

            write_sidecar_for(&make_dir_album(
                dir.path().to_string_lossy().into_owned(),
                None,
            ))
            .expect("second write");
            assert_eq!(
                properties_of(&sidecar).get("XMP-dc:Title"),
                None,
                "a cleared title must be removed, or the clear would revert on the next read"
            );
        }
    }
}
