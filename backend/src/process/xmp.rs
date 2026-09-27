//! Native-field extraction: `ExifTool` records → the app's own fields.
//!
//! The module is two layers, and the split is what makes the contract testable:
//!
//! * the **read layer** ([`native_metadata_for`], [`read_xmp_packet`]) answers
//!   "which file do I read" — that is where the sidecar rule lives, because it
//!   is a decision about file precedence, not about what a tag means;
//! * the **mapping layer** ([`map_native_fields`]) answers "which value wins",
//!   is pure, and is what the unit tests drive with recorded `ExifTool -j -G1`
//!   payloads.
//!
//! Parsing is `ExifTool`'s job: it locates the container (`APP1`, `APP13`, PNG
//! text chunks, `zTXt`/`iTXt` compression included) and this module only maps
//! the groups it reports. No hand-written metadata parser lives here.

use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::process::exif::read_metadata_record;

/// The metadata groups `ExifTool` reports the app's fields from, as the
/// family-1 (`-G1`) group names its output keys carry.
///
/// A family-1 group name follows the namespace, not the file: `dc:*` is always
/// `XMP-dc`, the XMP basic namespace `xmp:*` is `XMP-xmp`, IIM records are
/// `IPTC`, and a PNG text chunk is `PNG`. Measured on `ExifTool` 13.59 — a group
/// also exists per used namespace (`XMP-x` for the `x:` prefix above the packet,
/// `XMP-photoshop`, `XMP-microsoft`, …), which is why `xmp:Rating` and
/// `XMP-microsoft:RatingPercent` are not interchangeable.
const XMP_DC: &str = "XMP-dc";
const XMP_XMP: &str = "XMP-xmp";
const IPTC: &str = "IPTC";
const PNG: &str = "PNG";

/// `XMP-dc:Subject` — the `rdf:Bag` of keywords.
const SUBJECT: &str = "Subject";
/// `XMP-dc:Description` — the `rdf:Alt` caption.
const DESCRIPTION: &str = "Description";
/// `XMP-dc:Title`.
const TITLE: &str = "Title";
/// `XMP-xmp:Rating` — the only rating `ExifTool` reports; IPTC IIM has no
/// rating dataset and a PNG text chunk has no concept of one.
const RATING: &str = "Rating";
/// `IPTC:Keywords` — IIM dataset 2:25, repeatable.
const KEYWORDS: &str = "Keywords";
/// `IPTC:Caption-Abstract` — IIM dataset 2:120, the caption.
const CAPTION_ABSTRACT: &str = "Caption-Abstract";
/// `IPTC:ObjectName` — IIM dataset 2:05, the editorial title. Not
/// `IPTC:Headline` (2:105): see [`map_native_fields`].
const OBJECT_NAME: &str = "ObjectName";

/// The fields the app models natively, resolved for one asset.
///
/// Whatever else `ExifTool` reports is not this struct's business; the
/// read-only "further data" bucket that collects it is a later change.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct NativeMetadata {
    pub tags: HashSet<String>,
    pub description: Option<String>,
    /// 0–5 per `XMP-xmp:Rating`; a value outside the scale — including `-1`
    /// ("rejected" in some tools) — is no rating.
    pub rating: Option<u8>,
    /// `XMP-dc:Title`. Used for the album display name override.
    pub title: Option<String>,
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

/// Resolve the native fields of the image at `path`, from a record the caller
/// has already read.
///
/// `image` is the image's own `ExifTool` record — normally the one
/// [`crate::process::exif::read_metadata_record`] already read for the `exifVec`
/// map, so that both consumers share one engine call. Sharing is why the record
/// is a parameter and not read here: a second read of the same file would double
/// the metadata cost of indexing an image, which is the dominant cost at library
/// scale.
///
/// The sidecar is the exception, and the exception is one extra read: a sidecar
/// is a second file, and its XMP replaces the image's own packet while the
/// image's IPTC and text chunks keep filling what that packet left empty (see
/// [`map_native_fields`]). Any failure to read is the documented non-fallible
/// contract, as it is for `exifVec`: a file `ExifTool` cannot parse yields
/// empty fields rather than an error, so a damaged image still indexes.
pub fn native_metadata_for(path: &Path, image: Option<&Value>) -> NativeMetadata {
    // A sidecar that exists is authoritative for the *XMP* fields whether or not
    // it parses: the sidecar is where the app writes metadata back
    // (`PUT /put/edit_tag`), so falling back to the packet still inside the
    // image would undo the edit. Its presence is what takes the XMP source away
    // — not its content, and not its readability.
    let sidecar = discover_sidecar(path);
    let sidecar_record = sidecar
        .as_deref()
        .and_then(|sidecar| read_metadata_record(sidecar).ok());
    let xmp = match &sidecar_record {
        Some(record) => XmpSource::Record(record),
        // A sidecar that exists but yields no record — corrupt markup, or a
        // read that failed — still takes the XMP source away from the image.
        None if sidecar.is_some() => XmpSource::Unavailable,
        None => image.map_or(XmpSource::Unavailable, XmpSource::Record),
    };
    map_native_fields(&xmp, image)
}

/// Resolve the native fields of a standalone XMP packet — a `.xmp` sidecar read
/// as an asset in its own right, which is what a dir-album's `.albuminfo.xmp`
/// is. Such a file carries no IPTC and no text chunks, so only its XMP counts.
pub fn read_xmp_packet(path: &Path) -> NativeMetadata {
    match read_metadata_record(path) {
        Ok(record) => map_native_fields(&XmpSource::Record(&record), None),
        Err(_) => NativeMetadata::default(),
    }
}

/// The record the XMP-family fields are read from, or the fact that there is
/// none.
///
/// Which file that record came from is the read layer's decision, made before
/// the mapping runs; the mapping itself only sees a record or its absence. The
/// absence is not the same as "no XMP anywhere": a sidecar that exists and
/// yields nothing passes [`Unavailable`], not the image's own packet.
enum XmpSource<'a> {
    Record(&'a Value),
    Unavailable,
}

impl<'a> XmpSource<'a> {
    fn record(&self) -> Option<&'a Value> {
        match self {
            Self::Record(record) => Some(record),
            Self::Unavailable => None,
        }
    }
}

/// Map `ExifTool` records onto [`NativeMetadata`]. Pure: this is the function
/// the unit tests drive with recorded payloads.
///
/// # The mapping
///
/// | field         | 1st                            | 2nd                                | 3rd               |
/// | ------------- | ------------------------------ | ---------------------------------- | ----------------- |
/// | `description` | `XMP-dc:Description`           | `IPTC:Caption-Abstract` (2:120)    | `PNG:Description` |
/// | `title`       | `XMP-dc:Title`                 | `IPTC:ObjectName` (2:05)           | `PNG:Title`       |
/// | `rating`      | `XMP-xmp:Rating`               | —                                  | —                 |
/// | `tags`        | `XMP-dc:Subject` ∪ `IPTC:Keywords` (2:25) | —                     | —                 |
///
/// The scalars are **first non-empty wins**, in the order XMP → IPTC → text, so
/// a file that carries the field twice in two families is described by the one
/// the app would have written. A value that is present but blank does not count
/// as supplied: an empty `rdf:Bag` or an empty `rdf:Alt` is what a writer emits
/// when a field was cleared, and it must not shadow the family that still has
/// the value.
///
/// `title` takes IIM 2:05 `ObjectName` rather than 2:105 `Headline` because
/// 2:05 is the dataset the IPTC↔XMP mapping pairs with `dc:title` (2:105 pairs
/// with `XMP-photoshop:Headline`, a different property), and because that is
/// what `ExifTool`'s own MWG module pairs when it reconciles the two families;
/// snapfab's JPEG writer agrees, putting its title in both `dc:title` and
/// `ObjectName`.
///
/// `tags` is a **union** rather than a first-wins: `XMP-dc:Subject` and IIM 2:25
/// `Keywords` are two writings of one concept, and a file that has been through
/// two tools routinely carries the union of both. Ranking them would drop
/// keywords the user can see in the other family. PNG text contributes no
/// keywords, which is measured rather than assumed: `ExifTool` reports an
/// arbitrary text chunk under its own keyword (`PNG:ZKeyword` for a `zTXt`
/// chunk), and the PNG specification's registered text keywords — Title,
/// Author, Description, Copyright, Creation Time, Software, Disclaimer, Warning,
/// Source, Comment — contain no keyword concept. A writer that puts a keyword
/// list in a `Keywords` text chunk gets one string back
/// (`"alpha, beta"`), which cannot be split into tags without inventing a
/// boundary, so it is left out of the index rather than guessed at.
fn map_native_fields(xmp: &XmpSource<'_>, image: Option<&Value>) -> NativeMetadata {
    let record = xmp.record();
    NativeMetadata {
        tags: keywords(record, image),
        description: first_text(&[
            field(record, XMP_DC, DESCRIPTION),
            field(image, IPTC, CAPTION_ABSTRACT),
            field(image, PNG, DESCRIPTION),
        ]),
        rating: raw_field(record, XMP_XMP, RATING).and_then(rating_from),
        title: first_text(&[
            field(record, XMP_DC, TITLE),
            field(image, IPTC, OBJECT_NAME),
            field(image, PNG, TITLE),
        ]),
    }
}

/// The keyword carriers, unioned. See [`map_native_fields`] for why this is the
/// one field that is not first-wins.
fn keywords(xmp: Option<&Value>, image: Option<&Value>) -> HashSet<String> {
    let mut tags = HashSet::new();
    tags.extend(keyword_items(raw_field(xmp, XMP_DC, SUBJECT)));
    tags.extend(keyword_items(raw_field(image, IPTC, KEYWORDS)));
    tags
}

/// The first supplied value, in the order given. A blank never reaches this:
/// [`field`] drops it, so "present but empty" cannot shadow a lower family.
fn first_text(candidates: &[Option<String>]) -> Option<String> {
    candidates.iter().flatten().next().cloned()
}

/// Look one tag up in a record: `group` + `tag` in the `-G1` key space.
fn raw_field<'a>(record: Option<&'a Value>, group: &str, tag: &str) -> Option<&'a Value> {
    record?.get(format!("{group}:{tag}"))
}

/// One tag as text, trimmed, with blanks dropped so "present but empty" cannot
/// be mistaken for a value.
fn field(record: Option<&Value>, group: &str, tag: &str) -> Option<String> {
    let text = raw_field(record, group, tag)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The items of a list-valued tag, in the two shapes `ExifTool` reports one.
///
/// A list of more than one item is a JSON array; a list of exactly one is a
/// bare string, because `ExifTool` only writes an array when there is more than
/// one value (measured: `XMP-dc:Subject` is `["a", "b"]` for two keywords and
/// `"a"` for one). Blank items are dropped — an empty `rdf:Bag` comes back as
/// the empty string, not as an empty array.
fn keyword_items(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(keyword_item).collect(),
        Some(single) => keyword_item(single).into_iter().collect(),
        None => Vec::new(),
    }
}

/// One keyword, trimmed, with blanks dropped — an empty `rdf:Bag` arrives as
/// the empty string rather than as an empty list.
fn keyword_item(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Parse `XMP-xmp:Rating` into the app's 0–5 scale.
///
/// Measured shapes of the value on `ExifTool` 13.59: a packet holding an integer
/// yields a JSON number (`4`), and a packet holding anything else yields that
/// text as a string — `4 stars`, `3.5`, `not-a-number`, `""` — because the tag
/// carries no print conversion of its own. A rating is therefore an integer in
/// `0..=5`; `4 stars` is read as `4` (a unit some tools append), and `3.5`, a
/// word, a blank and a negative (`-1` is "rejected" in some tools) are not
/// ratings.
fn rating_from(value: &Value) -> Option<u8> {
    let text = match value {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.trim().to_owned(),
        _ => return None,
    };
    let digits_end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, rest) = text.split_at(digits_end);
    if digits.is_empty() || !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    let rating: u8 = digits.parse().ok()?;
    (rating <= 5).then_some(rating)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    // ── Recorded payloads ──────────────────────────────────────────────────
    //
    // Every payload below is an `exiftool -j -G1` record as ExifTool 13.59
    // reports it, trimmed to the groups the mapping reads. The comment on each
    // names the file it was recorded from, so a future change in `ExifTool`'s
    // output shows up as a failing shape test rather than as a silently
    // different mapping.

    /// `photo.xmp`, a sidecar in the form `xmp_write` produces.
    fn recorded_sidecar() -> Value {
        json!({
            "SourceFile": "photo.xmp",
            "ExifTool:ExifToolVersion": 13.59,
            "File:FileType": "XMP",
            "XMP-dc:Title": "Custom Title",
            "XMP-dc:Subject": "hiking",
            "XMP-dc:Description": "A trip",
            "XMP-xmp:Rating": 5
        })
    }

    /// `tagged.jpg`, a JPEG written by snapfab with `tags: [...]`: the XMP
    /// packet and the IIM record in one file, carrying the same keyword set,
    /// the same title and the same caption.
    fn recorded_tagged_jpeg() -> Value {
        json!({
            "SourceFile": "tagged.jpg",
            "File:FileType": "JPEG",
            "XMP-dc:Subject": ["grouped_alpine", "grouped_winter", "landscape"],
            "XMP-dc:Title": "Garden Bloom",
            "XMP-dc:Description": "Colorful flowers in full bloom.",
            "IFD0:ImageDescription": "grouped_alpine, grouped_winter [landscape]",
            "IPTC:ObjectName": "Garden Bloom",
            "IPTC:Keywords": ["grouped_alpine", "grouped_winter", "landscape"],
            "IPTC:Caption-Abstract": "Colorful flowers in full bloom."
        })
    }

    /// `iptc.jpg`, a JPEG carrying an IIM record and no XMP packet at all
    /// (written with `exiftool -IPTC:…`).
    fn recorded_iptc_only() -> Value {
        json!({
            "SourceFile": "iptc.jpg",
            "File:FileType": "JPEG",
            "IPTC:ObjectName": "IPTC Object Name",
            "IPTC:Headline": "IPTC Headline",
            "IPTC:Keywords": ["a", "b"],
            "IPTC:Caption-Abstract": "IPTC Caption",
            "IPTC:ApplicationRecordVersion": 4
        })
    }

    /// `alltext.png`, a PNG whose text chunks use keywords of their own — the
    /// standard ones plus arbitrary ones ExifTool reports verbatim.
    fn recorded_png_text_chunks() -> Value {
        json!({
            "SourceFile": "alltext.png",
            "File:FileType": "PNG",
            "PNG:Title": "The Title",
            "PNG:Author": "An Author",
            "PNG:Description": "A Description",
            "PNG:Comment": "A Comment",
            "PNG:Keywords": "alpha, beta",
            "PNG:ZKeyword": "zlib text value",
            "PNG:Rating": "4"
        })
    }

    /// `corrupt.xmp`, a sidecar of truncated markup. `ExifTool` reports a
    /// record with an `Error` and no metadata rather than failing the read.
    fn recorded_corrupt_sidecar() -> Value {
        json!({
            "SourceFile": "corrupt.xmp",
            "ExifTool:ExifToolVersion": 13.59,
            "ExifTool:Error": "File format error"
        })
    }

    // ── Mapping: scalars ───────────────────────────────────────────────────

    /// A sidecar's XMP packet supplies every field, because it is the XMP
    /// source and nothing else is needed.
    #[test]
    fn a_sidecar_packet_fills_every_native_field() {
        let data = map_native_fields(&XmpSource::Record(&recorded_sidecar()), None);
        assert_eq!(
            data,
            NativeMetadata {
                tags: HashSet::from(["hiking".to_string()]),
                description: Some("A trip".to_string()),
                rating: Some(5),
                title: Some("Custom Title".to_string()),
            }
        );
    }

    /// With no XMP source at all, an IIM record fills everything IIM carries —
    /// which is not the rating: IPTC IIM has no rating dataset, so `rating` has
    /// one source and no fallback.
    #[test]
    fn an_iptc_record_fills_what_iptc_carries() {
        let data = map_native_fields(&XmpSource::Unavailable, Some(&recorded_iptc_only()));
        assert_eq!(data.tags, HashSet::from(["a".to_string(), "b".to_string()]));
        assert_eq!(data.description.as_deref(), Some("IPTC Caption"));
        assert_eq!(data.title.as_deref(), Some("IPTC Object Name"));
        assert_eq!(data.rating, None);
    }

    /// PNG text chunks are the last resort, and they are reached only when
    /// neither XMP nor IPTC supplied the field. They carry no rating: a text
    /// chunk may be called anything, and `ExifTool` reports whatever keyword it
    /// finds, so a chunk named `Rating` is not a rating the app may read.
    #[test]
    fn png_text_chunks_fill_description_and_title_last() {
        let data = map_native_fields(&XmpSource::Unavailable, Some(&recorded_png_text_chunks()));
        assert_eq!(data.description.as_deref(), Some("A Description"));
        assert_eq!(data.title.as_deref(), Some("The Title"));
        assert_eq!(data.rating, None);
    }

    /// The precedence rule, both directions: when XMP supplies the field it
    /// wins, and when it supplies a *different* value for the same field the
    /// lower family is not consulted.
    #[test]
    fn xmp_supplies_a_field_in_preference_to_iptc_and_text() {
        let mut image = recorded_iptc_only();
        merge(&mut image, recorded_png_text_chunks());
        let mut xmp = recorded_sidecar();
        // The sidecar's own values, so the two families disagree on both fields.
        merge(
            &mut xmp,
            json!({ "XMP-dc:Title": "Packet Title", "XMP-dc:Description": "Packet caption" }),
        );

        let data = map_native_fields(&XmpSource::Record(&xmp), Some(&image));
        assert_eq!(data.title.as_deref(), Some("Packet Title"));
        assert_eq!(data.description.as_deref(), Some("Packet caption"));
    }

    /// A value that is present but blank is not a supplied value: a writer
    /// that cleared `dc:description` leaves an empty `rdf:Alt`, and the caption
    /// the file still carries in IIM must survive it.
    #[test]
    fn a_blank_xmp_value_falls_through_to_iptc() {
        let xmp = json!({ "XMP-dc:Title": "", "XMP-dc:Description": "   " });
        let data = map_native_fields(&XmpSource::Record(&xmp), Some(&recorded_iptc_only()));
        assert_eq!(data.title.as_deref(), Some("IPTC Object Name"));
        assert_eq!(data.description.as_deref(), Some("IPTC Caption"));
    }

    /// IPTC outranks PNG text for the same reason XMP outranks IPTC: the higher
    /// family is the one an edit in the app would have written.
    #[test]
    fn iptc_supplies_a_field_in_preference_to_png_text() {
        let mut image = recorded_iptc_only();
        merge(&mut image, recorded_png_text_chunks());
        let data = map_native_fields(&XmpSource::Unavailable, Some(&image));
        assert_eq!(data.title.as_deref(), Some("IPTC Object Name"));
        assert_eq!(data.description.as_deref(), Some("IPTC Caption"));
    }

    // ── Mapping: tags ──────────────────────────────────────────────────────

    /// Tags are the union of the two keyword families, not the first one found.
    /// The fixture is one where the two disagree, which is what a file carried
    /// through two tools looks like.
    #[test]
    fn tags_are_the_union_of_xmp_and_iptc() {
        let xmp = json!({ "XMP-dc:Subject": ["from_xmp", "shared"] });
        let image = json!({ "IPTC:Keywords": ["from_iptc", "shared"] });

        let data = map_native_fields(&XmpSource::Record(&xmp), Some(&image));
        assert_eq!(
            data.tags,
            HashSet::from([
                "from_xmp".to_string(),
                "from_iptc".to_string(),
                "shared".to_string()
            ])
        );
    }

    /// A keyword both families carry is one tag, not two. It is the property
    /// that makes the union a set of names rather than a concatenation.
    #[test]
    fn a_keyword_in_both_families_is_one_tag() {
        let xmp = json!({ "XMP-dc:Subject": ["alpha", "beta"] });
        let image = json!({ "IPTC:Keywords": ["alpha", "beta"] });

        let data = map_native_fields(&XmpSource::Record(&xmp), Some(&image));
        assert_eq!(
            data.tags,
            HashSet::from(["alpha".to_string(), "beta".to_string()])
        );
    }

    /// The fixture the tag scenarios are built on. snapfab writes the same
    /// keywords, title and caption into both families, so each scalar resolves
    /// to one value whichever family wins, the tag set is the union without
    /// duplicates, and the rating stays empty because neither family carries
    /// one.
    #[test]
    fn a_file_carrying_both_families_resolves_to_one_value_per_field() {
        let record = recorded_tagged_jpeg();
        let data = map_native_fields(&XmpSource::Record(&record), Some(&record));

        assert_eq!(
            data.tags,
            HashSet::from([
                "grouped_alpine".to_string(),
                "grouped_winter".to_string(),
                "landscape".to_string()
            ])
        );
        assert_eq!(
            data.description.as_deref(),
            Some("Colorful flowers in full bloom.")
        );
        assert_eq!(data.title.as_deref(), Some("Garden Bloom"));
        assert_eq!(data.rating, None);
    }

    /// A one-item list is a bare string in `ExifTool`'s output, and a tag with
    /// one keyword is the common case — a file with a single keyword must not
    /// lose it.
    #[test]
    fn a_single_valued_list_is_read_as_one_keyword() {
        let xmp = json!({ "XMP-dc:Subject": "only_xmp" });
        let image = json!({ "IPTC:Keywords": "only_iptc" });

        let data = map_native_fields(&XmpSource::Record(&xmp), Some(&image));
        assert_eq!(
            data.tags,
            HashSet::from(["only_xmp".to_string(), "only_iptc".to_string()])
        );
    }

    /// An empty `rdf:Bag` arrives as the empty string rather than as an empty
    /// array, and yields no tags either way.
    #[test]
    fn an_empty_keyword_list_yields_no_tags() {
        let xmp = json!({ "XMP-dc:Subject": "" });
        let image = json!({ "IPTC:Keywords": [] });

        let data = map_native_fields(&XmpSource::Record(&xmp), Some(&image));
        assert!(data.tags.is_empty());
    }

    /// Measured, not assumed: a `Keywords` text chunk is one string
    /// (`"alpha, beta"`), and splitting it on the comma would invent a boundary
    /// the file does not state. PNG text therefore contributes no tags.
    #[test]
    fn png_text_chunks_contribute_no_tags() {
        let data = map_native_fields(&XmpSource::Unavailable, Some(&recorded_png_text_chunks()));
        assert!(
            data.tags.is_empty(),
            "a PNG text chunk has no keyword concept: {tags:?}",
            tags = data.tags
        );
    }

    // ── Mapping: rating ────────────────────────────────────────────────────

    /// A rating is XMP's alone, and the number shape `ExifTool` prints for a
    /// well-formed packet is the JSON number the app's scale expects.
    #[test]
    fn a_rating_is_read_from_xmp() {
        let xmp = json!({ "XMP-xmp:Rating": 4 });
        assert_eq!(
            map_native_fields(&XmpSource::Record(&xmp), None).rating,
            Some(4)
        );
        assert_eq!(
            map_native_fields(&XmpSource::Record(&xmp), Some(&recorded_iptc_only())).rating,
            Some(4)
        );
    }

    /// A unit some tools append survives; everything that is not an integer in
    /// range is not a rating, `-1` ("rejected") included.
    #[test]
    fn only_an_integer_in_range_is_a_rating() {
        for (value, expected) in [
            (json!(0), Some(0)),
            (json!(5), Some(5)),
            (json!("4 stars"), Some(4)),
            (json!(" 3 "), Some(3)),
            (json!(6), None),
            (json!(-1), None),
            (json!("-1"), None),
            (json!(3.5), None),
            (json!("not-a-number"), None),
            (json!(""), None),
        ] {
            let record = json!({ "XMP-xmp:Rating": value.clone() });
            assert_eq!(
                map_native_fields(&XmpSource::Record(&record), None).rating,
                expected,
                "XMP-xmp:Rating {value} should map to {expected:?}"
            );
        }
    }

    // ── Mapping: absent and unreadable sources ─────────────────────────────

    /// With no record at all, every field is empty. This is the path a
    /// non-fallible caller lands on when `ExifTool` cannot read the file.
    #[test]
    fn no_records_yield_empty_metadata() {
        let empty = NativeMetadata::default();
        assert_eq!(map_native_fields(&XmpSource::Unavailable, None), empty);
    }

    /// A record with no metadata in it — what a corrupt sidecar reads as — is
    /// not an error and not a fallback: the XMP fields are simply absent.
    #[test]
    fn a_corrupt_sidecar_record_yields_no_xmp_fields() {
        let data = map_native_fields(
            &XmpSource::Record(&recorded_corrupt_sidecar()),
            None::<&Value>,
        );
        assert_eq!(data, NativeMetadata::default());
    }

    /// The sidecar rule this change settles (`.plan/exiftool-metadata-engine.md`
    /// decision 6): a sidecar's existence replaces the **XMP source only**. A
    /// corrupt sidecar therefore leaves the XMP fields empty *and* lets the
    /// image's own IIM record fill them — which is the opposite of the old pin
    /// that a corrupt sidecar suppresses everything the file carries.
    #[test]
    fn a_corrupt_sidecar_still_lets_the_images_iptc_fill() {
        let data = map_native_fields(
            &XmpSource::Record(&recorded_corrupt_sidecar()),
            Some(&recorded_iptc_only()),
        );
        assert_eq!(data.tags, HashSet::from(["a".to_string(), "b".to_string()]));
        assert_eq!(data.description.as_deref(), Some("IPTC Caption"));
        assert_eq!(data.title.as_deref(), Some("IPTC Object Name"));
    }

    /// A record that has neither of the families yields empty rather than
    /// erroring, so a damaged image still indexes.
    #[test]
    fn a_record_without_the_relevant_groups_yields_empty_metadata() {
        let record = json!({ "File:FileType": "JPEG", "IFD0:Make": "Canon" });
        assert_eq!(
            map_native_fields(&XmpSource::Record(&record), Some(&record)),
            NativeMetadata::default()
        );
    }

    /// Merge one record's keys into another, so a fixture can carry two
    /// families the way one file does.
    fn merge(into: &mut Value, other: Value) {
        match (into, other) {
            (Value::Object(into), Value::Object(other)) => into.extend(other),
            _ => panic!("recorded payloads are objects"),
        }
    }

    // ── Read layer: files, through `ExifTool` ──────────────────────────────
    //
    // Everything above drives the mapping with recorded payloads. These tests
    // go through the real reader, so they also pin that the payloads above are
    // the shape the engine really produces.

    /// A sidecar as `xmp_write` writes it: the packet the app edits through.
    fn sidecar_packet(
        tags: &[&str],
        description: Option<&str>,
        rating: Option<u8>,
        title: Option<&str>,
    ) -> String {
        let mut out = String::from(
            "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
             <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
             <rdf:Description rdf:about=\"\"\n\
             \x20   xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
             \x20   xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\n",
        );
        if let Some(title) = title {
            out.push_str(&format!(
                "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{title}</rdf:li></rdf:Alt></dc:title>\n"
            ));
        }
        if !tags.is_empty() {
            out.push_str("<dc:subject><rdf:Bag>\n");
            for tag in tags {
                out.push_str(&format!("  <rdf:li>{tag}</rdf:li>\n"));
            }
            out.push_str("</rdf:Bag></dc:subject>\n");
        }
        if let Some(description) = description {
            out.push_str(&format!(
                "<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{description}</rdf:li></rdf:Alt></dc:description>\n"
            ));
        }
        if let Some(rating) = rating {
            out.push_str(&format!("<xmp:Rating>{rating}</xmp:Rating>\n"));
        }
        out.push_str("</rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>\n");
        out
    }

    /// A JPEG written by snapfab with `tags`, which puts the keywords in both
    /// an XMP packet and an IIM record.
    fn snapfab_jpeg(dir: &Path, name: &str, tags: &[&str]) -> std::path::PathBuf {
        let photo = dir.join(name);
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(photo.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: Some(tags.iter().map(|tag| (*tag).to_string()).collect()),
            exif_date: Some("2024:05:06 07:08:09".into()),
            minimal: false,
        }])
        .expect("generate photo");
        photo
    }

    /// The same, as a PNG — which carries no IIM record, so a PNG fixture can
    /// isolate the XMP and text-chunk families.
    fn snapfab_png(dir: &Path, name: &str) -> std::path::PathBuf {
        let photo = dir.join(name);
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(photo.to_string_lossy().into_owned()),
            format: Some("png".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate photo");
        photo
    }

    /// The sidecar replaces the XMP source, and the image's IIM record still
    /// fills in the union: the sidecar's keyword and the image's own are both
    /// indexed, while the scalars come from the packet the app would have
    /// written.
    #[test]
    fn a_sidecar_replaces_the_xmp_source_and_the_image_iptc_still_fills() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = snapfab_jpeg(dir.path(), "photo.jpg", &["image_iptc_keyword"]);
        std::fs::write(
            dir.path().join("photo.xmp"),
            sidecar_packet(
                &["sidecar_xmp_keyword"],
                Some("sidecar description"),
                Some(4),
                Some("sidecar title"),
            ),
        )
        .expect("write sidecar");

        let image = read_metadata_record(&photo).expect("read image");
        let data = native_metadata_for(&photo, Some(&image));

        assert!(data.tags.contains("sidecar_xmp_keyword"), "{:?}", data.tags);
        assert!(
            data.tags.contains("image_iptc_keyword"),
            "the image's own IIM record still contributes: {:?}",
            data.tags
        );
        assert_eq!(data.description.as_deref(), Some("sidecar description"));
        assert_eq!(data.rating, Some(4));
        assert_eq!(data.title.as_deref(), Some("sidecar title"));
    }

    /// The rewritten pin, at the read layer: a corrupt sidecar no longer
    /// suppresses the whole file. It withholds the XMP fields (a sidecar that
    /// exists is the XMP source), and the image's IIM record fills them from
    /// its own metadata. See
    /// `.plan/exiftool-metadata-engine.md` decision 6; the scenario
    /// `corrupt_xmp_sidecar_suppresses_embedded_xmp` pins the same outcome
    /// end-to-end.
    #[test]
    fn a_corrupt_sidecar_withholds_xmp_but_not_the_images_iptc() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = snapfab_jpeg(dir.path(), "photo.jpg", &["image_iptc_keyword"]);
        // Truncated markup: the opening tags are there, the closing ones are not.
        std::fs::write(
            dir.path().join("photo.xmp"),
            "<x:xmpmeta><rdf:RDF><dc:subject><rdf:Bag><rdf:li>sidecar_keyword",
        )
        .expect("write corrupt sidecar");

        let image = read_metadata_record(&photo).expect("read image");
        let data = native_metadata_for(&photo, Some(&image));

        assert!(
            data.tags.contains("image_iptc_keyword"),
            "the image's IIM record is not suppressed by the sidecar: {:?}",
            data.tags
        );
        assert!(
            !data.tags.contains("sidecar_keyword"),
            "a corrupt packet contributes nothing: {:?}",
            data.tags
        );
        // snapfab writes the same caption in both families, so this value is
        // shared; what it pins is that the image's own record is still read.
        assert!(data.description.is_some());
    }

    /// A sidecar that exists but cannot be read does not hand the XMP source
    /// back to the image either — a directory named `photo.xmp` is the portable
    /// way to make the read fail, unlike a permission bit, which root ignores.
    /// The packet embedded in the PNG is therefore invisible, and the text
    /// chunks — which the image keeps filling from — are what remains.
    #[test]
    fn an_unreadable_sidecar_keeps_the_embedded_packet_out() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = png_with_text_chunks(
            dir.path(),
            "photo.png",
            &[text_chunk(b"Description", b"from the text chunk")],
            Some(embedded_xmp_packet()),
        );
        std::fs::create_dir(dir.path().join("photo.xmp")).expect("directory as sidecar");

        let image = read_metadata_record(&photo).expect("read image");
        let data = native_metadata_for(&photo, Some(&image));

        assert_eq!(data.description.as_deref(), Some("from the text chunk"));
        assert!(
            !data.tags.contains("embedded_keyword"),
            "the embedded packet is not the XMP source while a sidecar exists: {:?}",
            data.tags
        );
    }

    /// The reversed pin: a PNG carrying a **deflate-compressed** XMP packet in
    /// an `iTXt` text chunk yields its keywords and caption. The retired byte
    /// scan decompressed nothing, so this file carried no metadata as far as
    /// the app was concerned and
    /// `pinned_png_compressed_embedded_xmp_is_not_extracted` pinned that; the
    /// engine swap makes the packet readable, so the contract is now the
    /// positive one it should have been
    /// (`.plan/exiftool-metadata-engine.md` decision 6).
    ///
    /// Not vacuous: the fixture is a real PNG whose packet is not in plaintext,
    /// and the tags asserted are the two the packet carries.
    #[test]
    fn a_png_with_a_compressed_itxt_packet_yields_its_tags() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = png_with_text_chunks(dir.path(), "photo.png", &[], Some(embedded_xmp_packet()));
        let bytes = std::fs::read(&photo).expect("read png");
        assert!(
            !bytes
                .windows(b"<dc:subject>".len())
                .any(|window| window == b"<dc:subject>"),
            "the fixture must not leave the packet in plaintext"
        );

        let image = read_metadata_record(&photo).expect("read png");
        let data = native_metadata_for(&photo, Some(&image));

        assert_eq!(
            data.tags,
            HashSet::from([
                "embedded_keyword".to_string(),
                "second_embedded_keyword".to_string()
            ])
        );
        assert_eq!(data.description.as_deref(), Some("An embedded caption"));
    }

    /// Without a sidecar, the image's own packet is the XMP source — the
    /// ordinary path for an image that was never edited.
    #[test]
    fn a_file_without_a_sidecar_reads_its_own_packet() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = snapfab_jpeg(dir.path(), "photo.jpg", &["own_keyword"]);

        assert_eq!(discover_sidecar(&photo), None);
        let image = read_metadata_record(&photo).expect("read image");
        let data = native_metadata_for(&photo, Some(&image));

        assert!(data.tags.contains("own_keyword"), "{:?}", data.tags);
        assert!(data.description.is_some());
    }

    /// PNG text chunks reach the fields when nothing else carries them. The
    /// fixture has no XMP packet and no IIM record, so the description can only
    /// have come from the text chunk.
    #[test]
    fn a_png_text_chunk_fills_the_description_on_its_own() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = png_with_text_chunks(
            dir.path(),
            "photo.png",
            &[
                text_chunk(b"Description", b"A text description"),
                text_chunk(b"Title", b"A text title"),
            ],
            None,
        );

        let image = read_metadata_record(&photo).expect("read image");
        let data = native_metadata_for(&photo, Some(&image));

        assert_eq!(data.description.as_deref(), Some("A text description"));
        assert_eq!(data.title.as_deref(), Some("A text title"));
        assert!(data.tags.is_empty());
    }

    /// A read that cannot happen at all is the non-fallible contract: empty
    /// fields, no error. The indexer can reach this with a path that has been
    /// deleted since it was listed.
    #[test]
    fn an_absent_file_yields_empty_metadata() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            native_metadata_for(&dir.path().join("absent.jpg"), None),
            NativeMetadata::default()
        );
    }

    /// A dir-album's `.albuminfo.xmp` is a packet read as an asset of its own:
    /// no IIM, no text chunks, and the same field set as an image.
    #[test]
    fn a_standalone_packet_reads_its_own_xmp() {
        let dir = tempfile::tempdir().expect("temp dir");
        let packet = dir.path().join(".albuminfo.xmp");
        std::fs::write(
            &packet,
            sidecar_packet(&["hiking"], Some("A trip"), Some(5), Some("Custom Title")),
        )
        .expect("write packet");

        assert_eq!(
            read_xmp_packet(&packet),
            NativeMetadata {
                tags: HashSet::from(["hiking".to_string()]),
                description: Some("A trip".to_string()),
                rating: Some(5),
                title: Some("Custom Title".to_string()),
            }
        );
        assert_eq!(
            read_xmp_packet(&dir.path().join(".albuminfo-absent.xmp")),
            NativeMetadata::default()
        );
    }

    // ── PNG fixtures ───────────────────────────────────────────────────────
    //
    // Built here rather than pinned as files, because the contract under test
    // is a *container* one: a deflate-compressed XMP packet inside a PNG text
    // chunk, which the retired byte scan could not see at all. The image body
    // is snapfab's, so the result is a file `ExifTool` parses as a PNG; the
    // chunk carries its own CRC because a PNG chunk without one is malformed.

    /// The XMP packet the compressed-chunk tests embed.
    fn embedded_xmp_packet() -> &'static [u8] {
        br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:subject><rdf:Bag><rdf:li>embedded_keyword</rdf:li><rdf:li>second_embedded_keyword</rdf:li></rdf:Bag></dc:subject><dc:description><rdf:Alt><rdf:li xml:lang="x-default">An embedded caption</rdf:li></rdf:Alt></dc:description></rdf:Description></rdf:RDF></x:xmpmeta>"#
    }

    /// A `tEXt` chunk: keyword, null separator, Latin-1 text.
    fn text_chunk(keyword: &[u8], text: &[u8]) -> (&'static [u8], Vec<u8>) {
        let mut data = keyword.to_vec();
        data.push(0x00);
        data.extend_from_slice(text);
        (b"tEXt", data)
    }

    /// An `iTXt` chunk holding a *deflate-compressed* XMP packet, in the form
    /// the PNG specification defines it: keyword, null, compression flag 1,
    /// compression method 0, empty language tag, empty translated keyword, null,
    /// then the compressed text.
    fn compressed_xmp_chunk(packet: &[u8]) -> (&'static [u8], Vec<u8>) {
        let mut data = Vec::from(&b"XML:com.adobe.xmp\0"[..]);
        data.extend_from_slice(&[1, 0]);
        data.push(0x00); // empty language tag
        data.push(0x00); // empty translated keyword
        data.extend_from_slice(&zlib_fixed_huffman(packet));
        (b"iTXt", data)
    }

    /// Write a PNG with `chunks` inserted before the image data, optionally
    /// carrying `xmp` as a compressed text chunk.
    fn png_with_text_chunks(
        dir: &Path,
        name: &str,
        chunks: &[(&'static [u8], Vec<u8>)],
        xmp: Option<&[u8]>,
    ) -> std::path::PathBuf {
        let photo = snapfab_png(dir, name);
        let bytes = std::fs::read(&photo).expect("read generated png");
        let mut out = Vec::from(&bytes[..8]);
        let mut at = 8;
        while at + 8 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
            let kind = &bytes[at + 4..at + 8];
            if kind == b"IDAT" {
                if let Some(packet) = xmp {
                    let (chunk_type, data) = compressed_xmp_chunk(packet);
                    out.extend_from_slice(&png_chunk(chunk_type, &data));
                }
                for (chunk_type, data) in chunks {
                    out.extend_from_slice(&png_chunk(chunk_type, data));
                }
            }
            out.extend_from_slice(&bytes[at..at + 12 + length as usize]);
            at += 12 + length as usize;
        }
        std::fs::write(&photo, &out).expect("write png with chunks");
        photo
    }

    /// One PNG chunk: length, type, data, CRC over type and data.
    fn png_chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::with_capacity(12 + data.len());
        chunk.extend_from_slice(
            &u32::try_from(data.len())
                .expect("chunk too large")
                .to_be_bytes(),
        );
        chunk.extend_from_slice(kind);
        chunk.extend_from_slice(data);
        chunk.extend_from_slice(&crc32(kind, data).to_be_bytes());
        chunk
    }

    /// CRC-32 as PNG defines it (the reflected zlib polynomial, initial and
    /// final value inverted), bitwise so no compression or checksum dependency
    /// is pulled in for a test fixture.
    fn crc32(kind: &[u8], data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffff_u32;
        for byte in kind.iter().chain(data) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
                crc = (crc >> 1) ^ mask;
            }
        }
        !crc
    }

    /// `data` as a zlib stream holding one fixed-Huffman DEFLATE block.
    ///
    /// Literals only: it does not shrink anything, but it is a genuine deflate
    /// stream that any inflater accepts, and the packet is not readable in the
    /// file's bytes. Cross-checked against `zlib.decompress`; no compression
    /// dependency is wanted here.
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

        fn push(&mut self, value: u32, len: u32) {
            self.acc |= value << self.pending;
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
