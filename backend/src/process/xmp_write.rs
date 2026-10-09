use crate::model::abstract_data::AbstractData;
use crate::process::sanitize::is_valid_xml_char;
use crate::process::xmp::{NS_PICASU, ensure_picasu_namespace_registered};
use log::warn;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use xmpkit::{XmpMeta, XmpValue, ns};

/// What a write does with `dc:title`.
#[derive(Clone, Copy)]
enum TitleEdit<'a> {
    /// Not the app's field for this item (assets): the property is left
    /// wherever whoever wrote the sidecar put it. No endpoint edits a photo's
    /// title, so naming it here would destroy data the app never authored.
    Unmanaged,
    /// Albums: set to this value, or removed when `None`/empty — an album
    /// with no custom title must have no `dc:title` at all, so clearing a
    /// title really clears it and the directory-derived default can never be
    /// baked in.
    Set(Option<&'a str>),
}

/// Write (create or overwrite) the XMP sidecar for `abstract_data` with its
/// current managed metadata fields:
/// - `dc:subject` (tags), `dc:description`, `xmp:Rating` for all types
/// - `dc:title` additionally for albums, but only when explicitly
///   user-set (`metadata.custom_title`) — the directory-name-derived
///   default in `metadata.title` must never be baked into the sidecar, or
///   it would freeze and survive a later directory rename instead of being
///   re-derived from the new name.
/// - `picasu:Trashed` (soft-delete state) for all types, present only while
///   the item is trashed: a rebuild reads the flag back from the sidecar, so
///   deleting the database undeletes only the cache, not the user's intent.
///   Restoring removes the property, for the same delete-on-clear reason
///   `custom_title: None` removes `dc:title` — "no marker" must keep meaning
///   "never was in the trash".
///
/// The write is a read-modify-write through xmpkit: the existing packet is
/// read with [`XmpMeta::parse`], only the managed fields above are set, and
/// [`XmpMeta::serialize_packet`] writes everything else back untouched —
/// properties in namespaces the app does not model survive every edit.
/// Sidecar packets never go through `XmpFile::open`/`XmpFile::save` (the
/// Iteration 0 spike): the packet-in-file scan cannot match a bare `.xmp`,
/// and `XmpFile::save` errors while truncating the file to 0 bytes.
/// Output is `<?xpacket?>`-wrapped and carries `XMPToolkit: xmpkit`; pins on
/// it must assert parsed properties, never packet bytes.
///
/// An existing sidecar that does not parse (malformed XML, not valid UTF-8)
/// is replaced with a clean managed packet and a warning is logged: an
/// unreadable packet must not block an edit, and there is nothing
/// salvageable in content no parser can read.
///
/// Images/videos write `{basename}.{ext}.xmp` alongside their asset file.
/// Albums write `.albuminfo.xmp` inside `dir_path`. Items with no path
/// (pruned or album) have nowhere on disk to write and are silently skipped
/// (Ok returned).
///
/// Uses an atomic temp-file + rename to avoid partial-write races.
/// Sidecar write failures are returned to the caller; callers should log and
/// treat them as non-fatal.
pub fn write_sidecar_for(abstract_data: &AbstractData) -> io::Result<()> {
    let (sidecar, title) = if let AbstractData::Album(album) = abstract_data {
        (
            Path::new(&album.metadata.dir_path).join(".albuminfo.xmp"),
            TitleEdit::Set(album.metadata.custom_title.as_deref()),
        )
    } else {
        let Some(file_entry) = abstract_data.path() else {
            return Ok(());
        };
        (
            Path::new(&file_entry.file).with_extension("xmp"),
            TitleEdit::Unmanaged,
        )
    };
    let mut meta = load_sidecar(&sidecar)?;
    apply_managed_fields(
        &mut meta,
        abstract_data.tag(),
        abstract_data.description(),
        abstract_data.rating(),
        title,
        abstract_data.is_trashed(),
    )?;
    let content = meta.serialize_packet().map_err(|e| xmp_error_to_io(&e))?;
    write_sidecar_content(&sidecar, &content)
}

/// Open the existing sidecar for the read-modify-write, or start from an
/// empty [`XmpMeta`] (xmpkit's mechanism for a sidecar that does not exist
/// yet — the managed fields are then set on it and the packet is created by
/// `serialize_packet`).
///
/// - Absent (`NotFound`) → a fresh empty meta; the sidecar gets created.
/// - Present but unreadable — `XmpMeta::parse` errors, or `read_to_string`
///   fails on non-UTF-8 bytes before any parser runs → warn and start from a
///   clean meta. The replacement is deliberate: refusing to write would let
///   a packet no parser can read silently block every edit, while the edit
///   itself is perfectly representable (the branch's replacement semantics;
///   the parse error is the refusal signal).
/// - Any other io error (permissions, the path being a directory, ...) →
///   propagated: that is not "the packet is unreadable" but "this write
///   cannot happen", and callers already treat it as best-effort.
fn load_sidecar(sidecar: &Path) -> io::Result<XmpMeta> {
    let text = match std::fs::read_to_string(sidecar) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(XmpMeta::new()),
        Err(e) if e.kind() == io::ErrorKind::InvalidData => {
            warn!(
                "Replacing unreadable XMP sidecar {}: not valid UTF-8 ({e})",
                sidecar.display()
            );
            return Ok(XmpMeta::new());
        }
        Err(e) => return Err(e),
    };
    match XmpMeta::parse(&text) {
        Ok(meta) => Ok(meta),
        Err(e) => {
            warn!(
                "Replacing unparseable XMP sidecar {}: {e}",
                sidecar.display()
            );
            Ok(XmpMeta::new())
        }
    }
}

/// Set the managed fields on `meta`, leaving every other property in
/// the packet untouched. Absent managed values remove their property — the
/// same "omitted fields are not written" contract the old packet formatter
/// had, now expressed as delete-on-clear instead of leave-out-on-build.
fn apply_managed_fields(
    meta: &mut XmpMeta,
    tags: &HashSet<String>,
    description: Option<&str>,
    rating: Option<u8>,
    title: TitleEdit<'_>,
    trashed: bool,
) -> io::Result<()> {
    // `dc:subject`: the exact new tag set. The bag is replaced wholesale, so
    // a tag removed in the edit cannot resurface from the previous packet.
    let mut subject: Vec<String> = tags
        .iter()
        .map(|t| sanitize_value(t))
        .filter(|t| !t.is_empty())
        .collect();
    subject.sort();
    if subject.is_empty() {
        meta.delete_property(ns::DC, "subject")
            .map_err(|e| xmp_error_to_io(&e))?;
    } else {
        let items = subject.into_iter().map(XmpValue::String).collect();
        meta.set_property(ns::DC, "subject", XmpValue::Array(items))
            .map_err(|e| xmp_error_to_io(&e))?;
    }

    set_or_remove_alt(meta, ns::DC, "description", description)?;
    match title {
        TitleEdit::Unmanaged => {}
        TitleEdit::Set(value) => set_or_remove_alt(meta, ns::DC, "title", value)?,
    }

    match rating {
        Some(r) => meta
            .set_property(ns::XMP, "Rating", XmpValue::String(r.to_string()))
            .map_err(|e| xmp_error_to_io(&e))?,
        None => meta
            .delete_property(ns::XMP, "Rating")
            .map_err(|e| xmp_error_to_io(&e))?,
    }

    // `picasu:Trashed`: the soft-delete flag, the one managed property in a
    // namespace of the app's own. Present while trashed, deleted on restore
    // — restoring writes `false` nowhere, because an absent property is what
    // "not in the trash" means to the read path (see `trashed_from_property`).
    if trashed {
        ensure_picasu_namespace_registered();
        meta.set_property(NS_PICASU, "Trashed", XmpValue::Boolean(true))
            .map_err(|e| xmp_error_to_io(&e))?;
    } else if meta.has_property(NS_PICASU, "Trashed") {
        ensure_picasu_namespace_registered();
        meta.delete_property(NS_PICASU, "Trashed")
            .map_err(|e| xmp_error_to_io(&e))?;
    }
    Ok(())
}

/// Set an `rdf:Alt` text property (`dc:description`, `dc:title`) to `value`,
/// or remove it when the app holds none (empty counts as none, matching the
/// old formatter's `filter(|v| !v.is_empty())`). The property is owned
/// wholesale: whatever shape or language entries were there are replaced by
/// the single `x-default` entry the read path expects, because the managed
/// value has to be what the app reads back.
fn set_or_remove_alt(
    meta: &mut XmpMeta,
    namespace: &str,
    property: &str,
    value: Option<&str>,
) -> io::Result<()> {
    // Drop first: `set_localized_text` refuses to overwrite a property that
    // exists but is not an `rdf:Alt` (e.g. a compact/attribute-form
    // `dc:description="…"`), and a delete-on-clear needs the removal anyway.
    meta.delete_property(namespace, property)
        .map_err(|e| xmp_error_to_io(&e))?;
    let Some(value) = value.map(sanitize_value).filter(|v| !v.is_empty()) else {
        return Ok(());
    };
    meta.set_localized_text(namespace, property, "", "x-default", &value)
        .map_err(|e| xmp_error_to_io(&e))
}

/// Drop characters XML 1.0 forbids — the same filter the hand-rolled writer
/// applied while escaping, so odd input cannot produce a packet that no
/// parser will read back.
fn sanitize_value(s: &str) -> String {
    s.chars().filter(|&c| is_valid_xml_char(c)).collect()
}

/// xmpkit errors are not io errors, but the sidecar write exposes a single
/// `io::Result` to its callers (no caller churn) — carry the message over.
fn xmp_error_to_io(e: &xmpkit::XmpError) -> io::Error {
    io::Error::other(format!("xmpkit: {e}"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use crate::model::response::FileEntry;
    use crate::process::xmp::{NS_PICASU, XmpData, extract_xmp_data_from_packet};
    use arrayvec::ArrayString;
    use xmpkit::{XmpMeta, XmpValue, ns};

    // ── Fixtures ─────────────────────────────────────────────────────────────

    /// A sidecar as some other tool would have written it: the managed fields
    /// the app models plus four properties in namespaces the app does not
    /// (modeled on the branch scenario's packet). The write path must treat
    /// everything outside the managed set as untouchable.
    const FOREIGN_PACKET: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about=""
  xmlns:dc="http://purl.org/dc/elements/1.1/"
  xmlns:xmp="http://ns.adobe.com/xap/1.0/"
  xmlns:xmpRights="http://ns.adobe.com/xap/1.0/rights/"
  xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
  xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
  xmlns:Iptc4xmpCore="http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/">
  <dc:subject><rdf:Bag>
  <rdf:li>seed_tag</rdf:li>
  </rdf:Bag></dc:subject>
  <xmp:Rating>2</xmp:Rating>
  <xmpRights:Marked>True</xmpRights:Marked>
  <photoshop:City>Zürich</photoshop:City>
  <tiff:Make>SeedCam</tiff:Make>
  <Iptc4xmpCore:Location>Alps</Iptc4xmpCore:Location>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
"#;

    fn make_image(
        file: &Path,
        tags: &[&str],
        description: Option<&str>,
        rating: Option<u8>,
    ) -> AbstractData {
        let id = ArrayString::from("cafe1234").expect("failed to create test ArrayString");
        let mut object = ObjectSchema::new(id, ObjectType::Image);
        object.tags = tags.iter().map(|t| t.to_string()).collect();
        object.description = description.map(str::to_owned);
        object.rating = rating;
        let mut metadata = ImageMetadata::new(0, 0, 0, "jpg".to_string());
        metadata.path = Some(FileEntry::new(file, 0));
        AbstractData::Image(ImageCombined { object, metadata })
    }

    /// The sidecar's bytes after a write (the file under test).
    fn written_packet(sidecar: &Path) -> String {
        std::fs::read_to_string(sidecar).expect("sidecar not readable")
    }

    /// The managed fields of the written sidecar, read back through the
    /// production reader — the writer's output is only correct if the app's
    /// own read path sees it.
    fn written_data(sidecar: &Path) -> XmpData {
        extract_xmp_data_from_packet(written_packet(sidecar).as_bytes())
    }

    /// The written sidecar re-parsed through xmpkit itself. Panics if the
    /// packet is not well-formed.
    fn read_meta(sidecar: &Path) -> XmpMeta {
        XmpMeta::parse(&written_packet(sidecar)).expect("written sidecar must re-parse cleanly")
    }

    /// The four unmanaged properties seeded by `FOREIGN_PACKET`, still in
    /// place with their values.
    fn assert_foreign_properties_survive(meta: &XmpMeta) {
        assert!(
            meta.has_property(ns::XMP_RIGHTS, "Marked"),
            "xmpRights:Marked was dropped"
        );
        assert_eq!(
            meta.get_property(ns::PHOTOSHOP, "City"),
            Some(XmpValue::String("Zürich".to_string())),
            "photoshop:City was dropped or altered"
        );
        assert_eq!(
            meta.get_property(ns::TIFF, "Make"),
            Some(XmpValue::String("SeedCam".to_string())),
            "tiff:Make was dropped or altered"
        );
        assert_eq!(
            meta.get_property(ns::IPTC_CORE, "Location"),
            Some(XmpValue::String("Alps".to_string())),
            "Iptc4xmpCore:Location was dropped or altered"
        );
    }

    // ── Soft-delete marker ─────────────────────────────────────────────────────

    /// Trash state must survive a database rebuild, which the design doc's
    /// "generated state is rebuildable from the filesystem" rule requires, so
    /// the flag is written into the sidecar as a managed property: present
    /// when the item is trashed. Managed-field semantics still hold — the
    /// unmanaged (foreign) properties survive alongside it.
    #[test]
    fn trashed_item_writes_the_trash_marker() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");
        std::fs::write(&sidecar, FOREIGN_PACKET).expect("failed to seed sidecar");

        let mut image = make_image(&photo, &["kept"], Some("kept description"), Some(4));
        image.set_trashed(true);
        write_sidecar_for(&image).expect("failed to write sidecar");

        assert!(
            written_data(&sidecar).trashed,
            "the write path's output must read back as trashed through the production reader"
        );
        let meta = read_meta(&sidecar);
        assert!(
            meta.has_property(NS_PICASU, "Trashed"),
            "the trash marker must be in the packet, in the picasu namespace"
        );
        assert_eq!(
            meta.get_property(NS_PICASU, "Trashed"),
            Some(XmpValue::String("True".to_string())),
            "xmpkit 0.1.6 serializes a Boolean as the XMP wire form and reads \
             it back as a String, so the packet value is 'True' — the app's \
             own reader accepts it (see trashed_from_property)"
        );
        assert_foreign_properties_survive(&meta);
    }

    /// Restoring is the mirror image: an absent marker means not trashed, so a
    /// restore must *remove* the property rather than write `false` into it —
    /// otherwise the sidecar of every restored asset carries trash history
    /// forever, and "no marker" stops meaning "never was in the trash".
    #[test]
    fn restored_item_drops_the_trash_marker() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");
        let seeded = FOREIGN_PACKET
            .replace(
                "xmlns:dc=\"http://purl.org/dc/elements/1.1/\"",
                "xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n  xmlns:picasu=\"http://picasu.app/xmp/1.0/\"",
            )
            .replace(
                "<xmp:Rating>2</xmp:Rating>",
                "<xmp:Rating>2</xmp:Rating>\n  <picasu:Trashed>true</picasu:Trashed>",
            );
        std::fs::write(&sidecar, seeded).expect("failed to seed trashed sidecar");
        assert!(
            written_data(&sidecar).trashed,
            "the premise: the seeded sidecar reads as trashed"
        );

        let image = make_image(&photo, &["kept"], None, None);
        assert!(!image.is_trashed(), "the premise: the item is restored");
        write_sidecar_for(&image).expect("failed to write sidecar");

        assert!(!written_data(&sidecar).trashed);
        assert!(
            !read_meta(&sidecar).has_property(NS_PICASU, "Trashed"),
            "restore must delete the marker, not set it to false"
        );
        assert_foreign_properties_survive(&read_meta(&sidecar));
    }

    /// The common case leaves no trace: writing any field of an untrashed
    /// item must not introduce the picasu property (or the namespace).
    #[test]
    fn untrashed_item_writes_no_trash_marker() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");

        let image = make_image(&photo, &["kept"], None, None);
        write_sidecar_for(&image).expect("failed to write sidecar");

        assert!(!written_data(&sidecar).trashed);
        assert!(
            !read_meta(&sidecar).has_property(NS_PICASU, "Trashed"),
            "an untrashed item's sidecar must not claim it is trashed"
        );
    }

    // ── Read-modify-write pins ───────────────────────────────────────────────

    /// The core Iteration 2 contract: editing one managed field updates the
    /// managed set exactly (bag replaced, not appended) while every unmanaged
    /// property in the existing sidecar survives with its value. The writer
    /// this replaces rebuilt the packet from scratch and deleted all four
    /// foreign properties here.
    #[test]
    fn edit_updates_managed_fields_and_preserves_foreign_properties() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");
        std::fs::write(&sidecar, FOREIGN_PACKET).expect("failed to seed sidecar");

        let image = make_image(&photo, &["e2e_added"], Some("kept description"), Some(4));
        write_sidecar_for(&image).expect("failed to write sidecar");

        let data = written_data(&sidecar);
        assert_eq!(
            data.tags,
            HashSet::from(["e2e_added".to_string()]),
            "dc:subject must be the exact new tag set"
        );
        assert_eq!(data.description.as_deref(), Some("kept description"));
        assert_eq!(data.rating, Some(4));
        assert_foreign_properties_survive(&read_meta(&sidecar));
    }

    /// A photo's title is not a field the app edits (the old branch's
    /// `TitleEdit::Unmanaged`): an edit leaves a `dc:title` someone else put
    /// in the sidecar where it was, while the managed fields land.
    #[test]
    fn photo_title_is_left_where_someone_else_put_it() {
        let seeded = FOREIGN_PACKET.replace(
            "<xmp:Rating>2</xmp:Rating>",
            "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Somebody Else's Title</rdf:li></rdf:Alt></dc:title>\n  <xmp:Rating>2</xmp:Rating>",
        );
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");
        std::fs::write(&sidecar, seeded).expect("failed to seed sidecar");

        let image = make_image(&photo, &["edited"], Some("a description"), Some(3));
        write_sidecar_for(&image).expect("failed to write sidecar");

        let data = written_data(&sidecar);
        assert_eq!(data.title.as_deref(), Some("Somebody Else's Title"));
        assert_eq!(data.tags, HashSet::from(["edited".to_string()]));
        assert_eq!(data.description.as_deref(), Some("a description"));
        assert_eq!(data.rating, Some(3));
    }

    // ── Ported intents of the old `format_xmp_packet` tests (parse-based) ───

    /// No sidecar on disk → one is created carrying every managed field that
    /// is set.
    #[test]
    fn writes_all_managed_fields_when_no_sidecar_exists() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");

        let image = make_image(&photo, &["cat", "dog"], Some("nice photo"), Some(3));
        write_sidecar_for(&image).expect("failed to write sidecar");

        let data = written_data(&sidecar);
        assert_eq!(
            data.tags,
            HashSet::from(["cat".to_string(), "dog".to_string()])
        );
        assert_eq!(data.description.as_deref(), Some("nice photo"));
        assert_eq!(data.rating, Some(3));
    }

    /// Managed fields that are absent must be absent from the packet, not
    /// written as empty values.
    #[test]
    fn omits_absent_managed_fields() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");

        let image = make_image(&photo, &[], None, None);
        write_sidecar_for(&image).expect("failed to write sidecar");

        let meta = read_meta(&sidecar);
        assert!(!meta.has_property(ns::DC, "subject"));
        assert!(!meta.has_property(ns::DC, "description"));
        assert!(!meta.has_property(ns::XMP, "Rating"));
        assert!(!meta.has_property(ns::DC, "title"));
    }

    /// XML metacharacters in tags/description survive the write: the packet
    /// must re-parse and hand back the original strings.
    #[test]
    fn special_chars_round_trip() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");

        let image = make_image(
            &photo,
            &["a&b"],
            Some("desc <with> \"quotes\" & more"),
            None,
        );
        write_sidecar_for(&image).expect("failed to write sidecar");

        let data = written_data(&sidecar);
        assert_eq!(data.tags, HashSet::from(["a&b".to_string()]));
        assert_eq!(
            data.description.as_deref(),
            Some("desc <with> \"quotes\" & more")
        );
    }

    // ── Failure-path pins ────────────────────────────────────────────────────

    /// A sidecar that does not parse (`XmpMeta::parse` errors) must not block
    /// the edit: it is replaced with a clean managed packet and the write
    /// still reports success. The parse error is the refusal signal; the
    /// replacement also drops whatever unreadable content was in the file.
    #[test]
    fn replaces_unparseable_sidecar_with_a_clean_managed_packet() {
        // Not well-formed: `rdf:Description` is never closed before
        // `</rdf:RDF>` — quick-xml rejects the mismatched end tag.
        let malformed = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/">
<photoshop:City>Unreadable</photoshop:City>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
"#;
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");
        std::fs::write(&sidecar, malformed).expect("failed to seed sidecar");
        // The refusal signal itself: this content does not parse.
        assert!(
            XmpMeta::parse(malformed).is_err(),
            "fixture must exercise the parse-Err path"
        );

        let image = make_image(&photo, &["recovered"], Some("fresh"), Some(5));
        write_sidecar_for(&image).expect("an unparseable sidecar must not block the edit");

        let data = written_data(&sidecar);
        assert_eq!(data.tags, HashSet::from(["recovered".to_string()]));
        assert_eq!(data.description.as_deref(), Some("fresh"));
        assert_eq!(data.rating, Some(5));
        // Replaced, not partially salvaged: the foreign content of the
        // unreadable packet is gone with it.
        assert!(!written_packet(&sidecar).contains("Unreadable"));
    }

    /// Sidecar content that is not valid UTF-8 is unreadable by the same
    /// rule: `read_to_string` fails before any parser runs, so it takes the
    /// replacement path instead of blocking the edit on an io error that is
    /// really "this packet is not readable".
    #[test]
    fn replaces_non_utf8_sidecar_with_a_clean_managed_packet() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");
        std::fs::write(&sidecar, [0xFFu8, 0xFE, 0x00, 0xC3]).expect("failed to seed sidecar");

        let image = make_image(&photo, &["recovered"], None, Some(2));
        write_sidecar_for(&image).expect("a non-UTF-8 sidecar must not block the edit");

        let data = written_data(&sidecar);
        assert_eq!(data.tags, HashSet::from(["recovered".to_string()]));
        assert_eq!(data.rating, Some(2));
    }

    /// A truncated sidecar must not block the edit either.
    ///
    /// Observed with xmpkit 0.1.6 (pinned here test-first): cutting the
    /// packet before its closing tags parses leniently — `XmpMeta::parse`
    /// returns `Ok` and keeps every property whose bytes were fully written —
    /// while a cut *inside* a tag is a parse `Err` and takes the replacement
    /// path pinned above. This is the lenient half: the write proceeds
    /// normally on the parsed content, the managed fields land, and the file
    /// that comes out re-parses cleanly.
    #[test]
    fn truncated_sidecar_recovers_and_managed_fields_land() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let photo = dir.path().join("photo.jpg");
        let sidecar = dir.path().join("photo.xmp");
        let cut = FOREIGN_PACKET
            .find("</rdf:Description>")
            .expect("fixture shape");
        let truncated = &FOREIGN_PACKET[..cut];
        let observed = XmpMeta::parse(truncated)
            .expect("truncation before the closing tags must parse leniently");
        // Observed: the fully-written properties are all still there.
        assert_foreign_properties_survive(&observed);
        std::fs::write(&sidecar, truncated).expect("failed to seed sidecar");

        let image = make_image(&photo, &["recovered"], Some("after truncation"), Some(5));
        write_sidecar_for(&image).expect("a parseable truncated sidecar must not block the edit");

        let data = written_data(&sidecar);
        assert_eq!(data.tags, HashSet::from(["recovered".to_string()]));
        assert_eq!(data.description.as_deref(), Some("after truncation"));
        assert_eq!(data.rating, Some(5));
        // `read_meta` re-parses the file: the recovered packet is clean.
        assert_foreign_properties_survive(&read_meta(&sidecar));
    }

    mod album_sidecar {
        use super::*;
        use crate::model::album::{AlbumCombined, AlbumMetadata};

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
            let data = written_data(&sidecar_path);
            assert_eq!(data.title.as_deref(), Some("Vacation 2024"));
            assert_eq!(data.description.as_deref(), Some("A lovely trip"));
            assert_eq!(data.tags, HashSet::from(["vacation".to_string()]));
            assert_eq!(data.rating, Some(4));
        }

        /// Albums carry their sidecar at `.albuminfo.xmp`, and a trashed album
        /// must record the state there or a rebuild restores the album to the
        /// albums view with its children.
        #[test]
        fn trashed_dir_album_writes_the_marker_in_albuminfo() {
            let dir = tempfile::tempdir().expect("failed to create temp dir");
            let mut album = make_dir_album(
                dir.path().to_string_lossy().into_owned(),
                Some("Vacation 2024".to_string()),
            );
            album.set_trashed(true);

            write_sidecar_for(&album).expect("failed to write album sidecar");

            let sidecar_path = dir.path().join(".albuminfo.xmp");
            let data = written_data(&sidecar_path);
            assert!(
                data.trashed,
                "the album sidecar must record the trashed album"
            );
            assert!(
                read_meta(&sidecar_path).has_property(NS_PICASU, "Trashed"),
                "the marker must be in .albuminfo.xmp, in the picasu namespace"
            );
            assert_eq!(
                data.title.as_deref(),
                Some("Vacation 2024"),
                "the managed album fields are unaffected by the marker"
            );
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
            assert!(!written_packet(&sidecar_path).contains("dc:title"));
            assert!(!written_packet(&sidecar_path).contains("Vacation 2024"));
            assert!(!read_meta(&sidecar_path).has_property(ns::DC, "title"));
        }
    }
}
