//! Native-field extraction: `ExifTool` records → the app's own fields.
//!
//! The module is two layers, and the split is what makes the contract testable:
//!
//! * the **read layer** ([`asset_metadata_for`], [`native_metadata_for`],
//!   [`read_xmp_packet`]) answers "which file do I read" — that is where the
//!   sidecar rule lives, because it is a decision about file precedence, not
//!   about what a tag means;
//! * the **mapping layer** ([`map_native_fields`], [`map_further_fields`])
//!   answers "which value wins" and "what is left over", are pure, and are what
//!   the unit tests drive with recorded `ExifTool -j -G1` payloads.
//!
//! Parsing is `ExifTool`'s job: it locates the container (`APP1`, `APP13`, PNG
//! text chunks, `zTXt`/`iTXt` compression included) and this module only maps
//! the groups it reports. No hand-written metadata parser lives here.

use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::process::exif::{json_value_to_string, read_metadata_record};

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
const IPTC2: &str = "IPTC2";
const IPTC3: &str = "IPTC3";
const PNG: &str = "PNG";

/// The family-1 group names one IIM record can appear under, in the order the
/// mapping reads them.
///
/// The same IIM datasets are reported under different names depending on where
/// the record was found: `IPTC` for the record in a format's standard place
/// (the `8BIM` resource `0x0404` of a JPEG's APP13 block, tag 33723 of a TIFF),
/// and a numbered name — `IPTC2`, `IPTC3`, … — for a record outside it. Measured
/// on `ExifTool` 13.59 against a JPEG whose APP13 block holds two `0x0404`
/// resources: the first is `IPTC`, the second `IPTC2`
/// (`a_jpeg_with_two_iim_records_reads_both_them`), and the same numbering is
/// what `ExifTool`'s own corpus exercises for `IPTC3`.
///
/// They are one family, not three, so all three names are read: a file whose
/// only record sits outside the standard place carries a caption and keywords
/// that belong in `description` and `tags`, and a name-only lookup loses them.
/// The order is the standard record first, which is also the priority
/// `ExifTool` itself gives a non-standard record (it marks one low-priority,
/// since it is the copy a tool appended rather than the file's own).
const IIM_GROUPS: &[&str] = &[IPTC, IPTC2, IPTC3];

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
/// Whatever else `ExifTool` reports is not this struct's business; it is the
/// read-only "further data" bucket that collects it, which travels beside this
/// one as [`AssetMetadata::further`].
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

/// One asset's metadata as the app reads it: the fields the app models
/// natively, plus the read-only bucket of everything else `ExifTool` reported.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AssetMetadata {
    /// What the app models: the values it stores, edits and searches by.
    pub native: NativeMetadata,
    /// What it does not model, keyed `Group:Tag` — see [`map_further_fields`].
    pub further: BTreeMap<String, String>,
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

/// Resolve the whole of one asset's metadata, from a record the caller has
/// already read: the natively modelled fields and the further-data bucket.
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
/// image's IIM and text chunks keep filling what that packet left empty (see
/// [`map_native_fields`]). Any failure to read is the documented non-fallible
/// contract, as it is for `exifVec`: a file `ExifTool` cannot parse yields
/// empty fields rather than an error, so a damaged image still indexes.
pub fn asset_metadata_for(path: &Path, image: Option<&Value>) -> AssetMetadata {
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
    map_asset_metadata(&xmp, image)
}

/// The natively modelled fields of the image at `path`, without the
/// further-data bucket.
///
/// `process_video_info` is the caller: a video's `exifVec` comes from `ffprobe`,
/// and which source would own a video's further-data bucket is a later decision,
/// so the video path stays on this narrower read for now. The image path uses
/// [`asset_metadata_for`] and stores both halves.
pub fn native_metadata_for(path: &Path, image: Option<&Value>) -> NativeMetadata {
    asset_metadata_for(path, image).native
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
/// `IPTC:` in that table is every group name in [`IIM_GROUPS`], not one of them:
/// a dataset is read from the record `ExifTool` numbered as well as from the one
/// it did not, so a file whose IIM record sits outside the standard place fills
/// the same fields.
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
            iim_field(image, CAPTION_ABSTRACT),
            field(image, PNG, DESCRIPTION),
        ]),
        rating: raw_field(record, XMP_XMP, RATING).and_then(rating_from),
        title: first_text(&[
            field(record, XMP_DC, TITLE),
            iim_field(image, OBJECT_NAME),
            field(image, PNG, TITLE),
        ]),
    }
}

/// Map the two projections of the records the read layer resolved: what the app
/// models, and what it does not. Pure — this and the two functions it calls are
/// what the unit tests drive.
fn map_asset_metadata(xmp: &XmpSource<'_>, image: Option<&Value>) -> AssetMetadata {
    AssetMetadata {
        native: map_native_fields(xmp, image),
        further: map_further_fields(xmp, image),
    }
}

/// Map the read-only **further data** bucket: every key the native mapping did
/// not consume, keyed `Group:Tag`.
///
/// `.plan/exiftool-metadata-engine.md` decision 3: everything `ExifTool` returns
/// that the app does not model is surfaced as simple key/value pairs, read-only.
/// The bucket is the API's `furtherMetadata` and the sidebar renders it as a
/// category; nothing edits it, because the app does not know what any of these
/// fields mean well enough to write one back.
///
/// # The split, and why each side is where it is
///
/// **In** — the three source families a writing tool puts human-readable values
/// into, minus what [`map_native_fields`] consumed:
///
/// * `XMP-*`, every namespace. The same family the native fields come from, so
///   the two are the same kind of thing: `XMP-xmp:CreatorTool`,
///   `XMP-photoshop:Credit`, `XMP-x:XMPToolkit` (which a writer stamps into the
///   packet, as it does its own EXIF `Software` tag). The namespace matters and
///   is kept: `xmp:Rating` and `XMP-microsoft:RatingPercent` are different
///   properties, and dropping the namespace would collide them.
/// * `IPTC`, `IPTC2`, `IPTC3` — the IIM record, under whichever group name
///   `ExifTool` filed it (see [`IIM_GROUPS`]). The native mapping reads all of
///   them, so the three datasets it consumes are excluded here under each name:
///   a numbered record's `Caption-Abstract` is in `description`, not repeated in
///   the bucket. That is the complement staying exact — a key consumed natively
///   is consumed whatever group the engine filed it in.
/// * `PNG`, the text chunks. See the exclusion below for the other half of that
///   group.
///
/// **Out** — everything else, each for a reason measured rather than assumed:
///
/// * the EXIF family (`IFD*`, `ExifIFD`, `InteropIFD`, `GPS`, `SubIFD*`) is
///   already the `exifVec` map, key for key. A key in two maps would have two
///   values and no rule for which one the user reads.
/// * `File:`, `System:` and `ExifTool:` are facts about the read, not about the
///   metadata: `FileSize`, `MIMEType`, `FileName`, `Directory`, the access and
///   modify timestamps — which would make the sidebar's content change whenever
///   the file is touched, and show the moment it was indexed — and
///   `ExifToolVersion`, which is a fact about the reader. `exifVec` already
///   excludes them for the same reason.
/// * `Composite:` holds values `ExifTool` *derived* from tags that are already
///   reported. Measured on a snapfab JPEG: `Composite:Aperture` 10.7 restates
///   `ExifIFD:FNumber` 10.7, `Composite:ShutterSpeed` `1/888` restates
///   `ExifIFD:ExposureTime` `1/888`, and `Composite:ImageSize` `2x2` restates
///   the dimensions the app models natively as `width`/`height`. The bucket is
///   for what a photographer or a tool wrote into the file; showing a computed
///   value beside its own source would be a second, differently-formatted
///   answer to a question the sidebar already answers.
/// * `JFIF`/`JFXX` are the JPEG container's own parameters (`JFIFVersion`,
///   `ResolutionUnit`, `XResolution`, `YResolution`) and a thumbnail blob, in
///   the same category as `File:`: container facts, not written metadata.
/// * the maker-note groups (`Canon`, `Nikon`, `Olympus`, …) are a vendor's
///   private re-reading of the EXIF directories, plus binary blobs. Same
///   duplication as the EXIF family, with worse legibility.
/// * `ICC_Profile`/`ICC-header`/… are a colour profile's internals — measured
///   40+ entries per file of matrix and TRC numbers, half of them
///   `(Binary data 2060 bytes, use -b option to extract)`. A payload, not
///   key/value metadata.
/// * `Photoshop:` (the image resource block) is the one group this measurement
///   left out on judgement rather than on category. It is written metadata —
///   `WriterName`, `ReaderName`, `URL`, `CopyrightFlag` — but it is an editing
///   tool's record of its own session rather than a metadata standard the app
///   or its native mapping reads, and 13 of the 21 entries `ExifTool` reports for
///   a Photoshop-edited JPEG are print/resolution settings. Recorded here as the
///   next candidate; widening the include list later is additive, and a
///   reindex is what fills a bucket for files indexed before.
///
/// # Two rules that keep the complement exact
///
/// * A key [`map_native_fields`] consumed is never repeated here, whatever
///   family it is in. [`NATIVE_KEYS`] is the single list of those keys, built
///   from the same group and tag constants the native mapping reads, so
///   renaming a constant cannot desynchronise the two.
/// * The XMP family is read from the XMP source record and the IIM and text
///   chunks from the image's own, exactly as the native fields are. A sidecar
///   therefore masks the image's own XMP packet here too, instead of
///   resurrecting the fields an edit replaced.
fn map_further_fields(xmp: &XmpSource<'_>, image: Option<&Value>) -> BTreeMap<String, String> {
    let mut further = BTreeMap::new();
    collect_further_from(xmp.record(), is_xmp_family, &mut further);
    collect_further_from(image, is_written_metadata_family, &mut further);
    further
}

/// Add every key of `record` whose family-1 group `wanted` accepts, and whose
/// key the native mapping or [`PNG_CONTAINER_PROPERTIES`] has not claimed, to
/// `further` under its own `Group:Tag` name.
///
/// The prefix is kept in the key rather than stripped as `exifVec` strips the
/// EXIF family's: the bucket deliberately holds more than one family, and
/// `Description` is a legitimate key name in both XMP and IIM. A key that says
/// where it came from cannot collide with one that does not, and the sidebar can
/// show the provenance instead of guessing at it.
fn collect_further_from(
    record: Option<&Value>,
    wanted: fn(&str) -> bool,
    further: &mut BTreeMap<String, String>,
) {
    let Some(Value::Object(entries)) = record else {
        return;
    };
    for (key, value) in entries {
        let Some((group, tag)) = key.split_once(':') else {
            continue;
        };
        if !wanted(group) || is_native_key(group, tag) || is_png_container_property(group, tag) {
            continue;
        }
        // A blank is dropped, on the same rule the native mapping applies to a
        // cleared `rdf:Alt` or `rdf:Bag`: `ExifTool` reports "no value" as
        // `null` in one tag and as the empty string in another (`ExifIFD:
        // UserComment` is the common one), and a bucket is a list of rows to
        // read, where a row with nothing in it is noise. The flattening itself is
        // `exifVec`'s, shared on purpose — see `json_value_to_string`; a list
        // arrives `", "`-joined because the bucket is `String -> String` and a
        // `XMP-dc:Subject` list is not a tag list here (the tag list was already
        // consumed natively, so this key only exists when it is not).
        match json_value_to_string(value) {
            Some(text) if !text.trim().is_empty() => {
                further.insert(key.clone(), text);
            }
            _ => {}
        }
    }
}

/// Every `(group, tag)` pair [`map_native_fields`] reads, as the bucket's
/// exclusion list.
///
/// Built from the same group and tag constants the native mapping uses rather
/// than from literals, so a renamed constant moves the exclusion with it and a
/// newly consumed key that is not added here shows up as the same key in both
/// places — which
/// `no_native_key_is_repeated_in_the_bucket` and the API-level scenario
/// `metadata_detail_exposes_further_data` both fail on. It is a `LazyLock`
/// because one entry depends on the other: the IIM datasets are consumed from
/// every group name in [`IIM_GROUPS`], so the list is their cross product rather
/// than three hand-copied triples.
static NATIVE_KEYS: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    let mut keys = vec![
        // description
        (XMP_DC, DESCRIPTION),
        (PNG, DESCRIPTION),
        // title
        (XMP_DC, TITLE),
        (PNG, TITLE),
        // rating
        (XMP_XMP, RATING),
        // tags
        (XMP_DC, SUBJECT),
    ];
    keys.extend(IIM_GROUPS.iter().flat_map(|group| {
        [
            // description
            (*group, CAPTION_ABSTRACT),
            // title
            (*group, OBJECT_NAME),
            // tags
            (*group, KEYWORDS),
        ]
    }));
    keys
});

/// The XMP family, matched by shape: one group per used namespace, plus a bare
/// `XMP` for a packet written in the XMP namespace itself.
fn is_xmp_family(group: &str) -> bool {
    group == "XMP" || group.starts_with("XMP-")
}

/// The IIM record — under whichever group name `ExifTool` filed it — and the
/// PNG text chunks, which share a group with the container's own properties.
fn is_written_metadata_family(group: &str) -> bool {
    IIM_GROUPS.contains(&group) || group == PNG
}

/// Whether the native mapping already owns this key.
fn is_native_key(group: &str, tag: &str) -> bool {
    NATIVE_KEYS
        .iter()
        .any(|(candidate_group, candidate_tag)| *candidate_group == group && *candidate_tag == tag)
}

/// The PNG container's own image properties, which `ExifTool` reports in the
/// same `PNG` group as the text chunks: the IHDR fields and the two rendering
/// chunks it names them for.
///
/// Measured on `ExifTool` 13.59 — the seven first entries are what every PNG
/// reports, the last two what `PNG.png` and `PGF.pgf` in `ExifTool`'s own corpus
/// add. They are excluded because they are the picture rather than the file's
/// metadata, and because `PNG:ImageWidth`/`ImageHeight` would be a second and a
/// third copy of the `width`/`height` the app already models natively.
///
/// The list is measured rather than exhaustive: a PNG variant `ExifTool` describes
/// with a property outside it would show that property in the bucket. That is
/// the cheaper error here — an extra visible row — than the alternative, an
/// allowlist of the PNG specification's registered text keywords, which would
/// silently drop every text chunk written under a keyword of the writer's own
/// choosing (`PNG:ZKeyword` for a `zTXt` chunk is a real one).
const PNG_CONTAINER_PROPERTIES: &[&str] = &[
    "ImageWidth",
    "ImageHeight",
    "BitDepth",
    "ColorType",
    "Compression",
    "Filter",
    "Interlace",
    "BackgroundColor",
    "SRGBRendering",
];

fn is_png_container_property(group: &str, tag: &str) -> bool {
    group == PNG && PNG_CONTAINER_PROPERTIES.contains(&tag)
}

/// The keyword carriers, unioned. See [`map_native_fields`] for why this is the
/// one field that is not first-wins. An IIM record's keywords are read from
/// every group name in [`IIM_GROUPS`], for the same reason the scalars are: the
/// numbered records hold the same dataset.
fn keywords(xmp: Option<&Value>, image: Option<&Value>) -> HashSet<String> {
    let mut tags = HashSet::new();
    tags.extend(keyword_items(raw_field(xmp, XMP_DC, SUBJECT)));
    for group in IIM_GROUPS {
        tags.extend(keyword_items(raw_field(image, group, KEYWORDS)));
    }
    tags
}

/// One IIM dataset as text, from the first record that supplies it. Blank values
/// do not count, exactly as in the XMP family, so a record that carries an empty
/// caption does not shadow the record that carries the real one.
fn iim_field(record: Option<&Value>, tag: &str) -> Option<String> {
    IIM_GROUPS
        .iter()
        .find_map(|group| field(record, group, tag))
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

    /// `two-records.jpg`, a JPEG whose APP13 block holds **two** Photoshop
    /// image resources of type `0x0404` — two IIM records in one file, as a
    /// tool that appends a record rather than replacing one produces.
    /// `ExifTool` files the first under `IPTC:` and numbers the rest, so the
    /// second is `IPTC2:`. Measured on 13.59 against a file built by the
    /// `two_iim_records_in_one_app13` helper below; the group naming itself is
    /// documented by ExifTool ("a number added for non-standard IPTC"),
    /// measured on its own corpus for `IPTC2` and `IPTC3`.
    fn recorded_iptc_numbered_records() -> Value {
        json!({
            "SourceFile": "two-records.jpg",
            "File:FileType": "JPEG",
            "IPTC:ApplicationRecordVersion": 4,
            "IPTC:ObjectName": "Standard record object name",
            "IPTC:Keywords": "standard_kw",
            "IPTC2:ApplicationRecordVersion": 4,
            "IPTC2:ObjectName": "Numbered record object name",
            "IPTC2:Keywords": ["numbered_kw", "second_numbered_kw"],
            "IPTC2:Caption-Abstract": "Numbered record caption",
            "IPTC2:City": "Numberedville"
        })
    }

    /// The three group names the same IIM datasets appear under, as a record
    /// with nothing in the standard location at all: `ExifTool` files a record
    /// it finds only outside the standard place under a numbered group, and a
    /// file may carry the third as well.
    fn recorded_iptc_all_three_groups() -> Value {
        json!({
            "SourceFile": "numbered-only.jpg",
            "File:FileType": "JPEG",
            "IPTC2:Caption-Abstract": "Caption in the second group",
            "IPTC2:Keywords": "kw2",
            "IPTC3:Caption-Abstract": "Caption in the third group",
            "IPTC3:Keywords": ["kw3"],
            "IPTC3:ObjectName": "Object name in the third group"
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

    /// An IIM record is an IIM record whatever `ExifTool` files it under. The
    /// datasets of a second (or third) record in the same file arrive as
    /// `IPTC2:`/`IPTC3:` and used to stop at the mapping's `IPTC:`-only lookup,
    /// so a file with nothing in the standard place had a caption and keywords
    /// no field could hold — they landed in the further-data bucket instead of
    /// in `description` and `tags`, which is the one thing that bucket is not for.
    #[test]
    fn an_iim_record_exiftool_files_as_iptc2_or_iptc3_fills_the_same_fields() {
        let data = map_native_fields(
            &XmpSource::Unavailable,
            Some(&recorded_iptc_all_three_groups()),
        );

        assert_eq!(
            data.description.as_deref(),
            Some("Caption in the second group")
        );
        assert_eq!(
            data.title.as_deref(),
            Some("Object name in the third group")
        );
        assert_eq!(
            data.tags,
            HashSet::from(["kw2".to_string(), "kw3".to_string()]),
            "keywords are a union, so a numbered record's add to it rather than \
             replace it: {:?}",
            data.tags
        );
        assert_eq!(data.rating, None);
    }

    /// The order the three groups are read in: the standard record first, then
    /// the numbered ones. `ExifTool` itself marks a non-standard record
    /// low-priority for the same reason — it is the record a tool appended, and
    /// the standard one is the file's own — so this is the engine's precedence,
    /// not a new one.
    #[test]
    fn the_standard_iim_record_outranks_a_numbered_one() {
        let data = map_native_fields(
            &XmpSource::Unavailable,
            Some(&recorded_iptc_numbered_records()),
        );

        assert_eq!(data.title.as_deref(), Some("Standard record object name"));
        // The standard record carries no caption here, so the numbered one
        // supplies it rather than the field staying empty.
        assert_eq!(data.description.as_deref(), Some("Numbered record caption"));
        assert_eq!(
            data.tags,
            HashSet::from([
                "standard_kw".to_string(),
                "numbered_kw".to_string(),
                "second_numbered_kw".to_string()
            ])
        );
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

    // ── Mapping: the further-data bucket ────────────────────────────────────

    /// Every group a `-G1` read reports that is not the file's own written
    /// metadata, plus the two families the native mapping reads. Recorded from
    /// the ExifTool 13.59 outputs named on each key's line in the test below.
    fn recorded_kitchen_sink() -> Value {
        json!({
            // Container facts, the read-time trivia, the tool's own version,
            // the container parameters, the derived values, the EXIF family,
            // and a vendor's private block.
            "File:FileType": "JPEG",
            "File:MIMEType": "image/jpeg",
            "File:ImageWidth": 2,
            "System:FileName": "kitchen-sink.jpg",
            "System:FileSize": "1602 bytes",
            "System:FileModifyDate": "2026-09-27 20:09:55",
            "ExifTool:ExifToolVersion": 13.59,
            "JFIF:JFIFVersion": 1.02,
            "JFIF:XResolution": 1,
            "Composite:Aperture": 10.7,
            "Composite:ImageSize": "2x2",
            "IFD0:Make": "Nikon",
            "ExifIFD:DateTimeOriginal": "2024-05-06 07:08:09",
            "MakerNotes:Macro": 0,
            // Every key the native mapping consumes.
            "XMP-dc:Title": "Garden Bloom",
            "XMP-dc:Description": "Colorful flowers in full bloom.",
            "XMP-dc:Subject": ["alpine", "winter"],
            "XMP-xmp:Rating": 5,
            "IPTC:ObjectName": "Garden Bloom",
            "IPTC:Caption-Abstract": "Colorful flowers in full bloom.",
            "IPTC:Keywords": ["alpine", "winter"],
            "PNG:Title": "A Title",
            "PNG:Description": "A Description",
            // The same three IIM datasets again, under the group names ExifTool
            // gives a record it found outside the standard place. All of them
            // are in the record so the complement test's control has something
            // to exclude in every group, and none of them may reach the bucket.
            "IPTC2:ObjectName": "Second record object name",
            "IPTC2:Caption-Abstract": "Second record caption",
            "IPTC2:Keywords": "stew",
            "IPTC3:ObjectName": "Third record object name",
            "IPTC3:Caption-Abstract": "Third record caption",
            "IPTC3:Keywords": "jambalaya",
            // Everything else in the three source families.
            "XMP-xmp:CreatorTool": "snapfab 1.0",
            "XMP-x:XMPToolkit": "Image::ExifTool 13.59",
            "XMP-photoshop:Credit": "picasu test suite",
            "IPTC:By-line": "Ada Lovelace",
            "IPTC:City": "London",
            "IPTC:ApplicationRecordVersion": 4,
            "IPTC2:City": "Secondville",
            "IPTC3:ApplicationRecordVersion": 4,
            "PNG:Comment": "A Comment",
            "PNG:ZKeyword": "zlib text value"
        })
    }

    /// The bucket is the exact complement of the native mapping, with the
    /// `Group:Tag` prefix kept so a key says which family it came from.
    ///
    /// The control comes first: every native key must really be in the record and
    /// really be consumed, or the "complement" would hold vacuously.
    #[test]
    fn the_bucket_is_the_complement_of_the_native_keys_with_the_prefix_kept() {
        let record = recorded_kitchen_sink();
        let native = map_native_fields(&XmpSource::Record(&record), Some(&record));

        // Control: the native mapping owns these, and they are all in the record.
        assert_eq!(native.title.as_deref(), Some("Garden Bloom"));
        assert_eq!(
            native.description.as_deref(),
            Some("Colorful flowers in full bloom.")
        );
        assert_eq!(native.rating, Some(5));
        assert_eq!(
            native.tags,
            HashSet::from([
                "alpine".to_string(),
                "winter".to_string(),
                "stew".to_string(),
                "jambalaya".to_string()
            ]),
            "keywords are a union across the IIM group names too"
        );
        for key in NATIVE_KEYS.iter() {
            let prefixed = format!("{}:{}", key.0, key.1);
            assert!(
                record.get(&prefixed).is_some(),
                "control: {prefixed} is not in the record, so excluding it proves nothing"
            );
        }

        let bucket = map_further_fields(&XmpSource::Record(&record), Some(&record));
        assert_eq!(
            bucket,
            BTreeMap::from([
                ("IPTC:ApplicationRecordVersion".to_string(), "4".to_string()),
                ("IPTC:By-line".to_string(), "Ada Lovelace".to_string()),
                ("IPTC:City".to_string(), "London".to_string()),
                ("IPTC2:City".to_string(), "Secondville".to_string()),
                (
                    "IPTC3:ApplicationRecordVersion".to_string(),
                    "4".to_string()
                ),
                ("PNG:Comment".to_string(), "A Comment".to_string()),
                ("PNG:ZKeyword".to_string(), "zlib text value".to_string()),
                (
                    "XMP-photoshop:Credit".to_string(),
                    "picasu test suite".to_string()
                ),
                (
                    "XMP-x:XMPToolkit".to_string(),
                    "Image::ExifTool 13.59".to_string()
                ),
                ("XMP-xmp:CreatorTool".to_string(), "snapfab 1.0".to_string()),
            ]),
            "the bucket is every source-family key the native mapping left, and nothing else"
        );
    }

    /// A key the native mapping consumed cannot appear in the bucket, whatever
    /// family it is in: the value would then exist in two places with no rule
    /// for which one the user is reading.
    #[test]
    fn no_native_key_is_repeated_in_the_bucket() {
        let record = recorded_kitchen_sink();
        let bucket = map_further_fields(&XmpSource::Record(&record), Some(&record));

        for (group, tag) in NATIVE_KEYS.iter() {
            let prefixed = format!("{group}:{tag}");
            assert!(
                !bucket.contains_key(&prefixed),
                "{prefixed} is consumed by the native mapping and must not be in the bucket: {bucket:?}"
            );
        }
        // The complement is by `(group, tag)`, not by tag name: a dataset the
        // mapping does not read still reaches the bucket in a numbered record,
        // under the group ExifTool filed it in, and a name-only exclusion would
        // drop it. The consumed names are excluded in every group instead —
        // `IPTC2:Caption-Abstract` and `IPTC3:Keywords` are asserted absent
        // above, and they are the same datasets `IPTC:Caption-Abstract` and
        // `IPTC:Keywords` are.
        assert_eq!(
            bucket.get("IPTC2:City").map(String::as_str),
            Some("Secondville")
        );
        assert_eq!(
            bucket.get("IPTC:City").map(String::as_str),
            Some("London"),
            "the same dataset in the standard record is a separate key"
        );
    }

    /// The groups that are not the file's own written metadata: the container
    /// (`File:`, `JFIF:`), the read-time trivia (`System:`), the reader's own
    /// version (`ExifTool:`), the values derived from tags already shown
    /// (`Composite:`), the EXIF family (already `exifVec`) and the vendor
    /// blocks (a private re-reading of EXIF).
    #[test]
    fn groups_that_are_not_written_metadata_stay_out_of_the_bucket() {
        let record = recorded_kitchen_sink();
        let bucket = map_further_fields(&XmpSource::Record(&record), Some(&record));

        for group in [
            "IFD0",
            "ExifIFD",
            "File",
            "System",
            "ExifTool",
            "JFIF",
            "Composite",
            "MakerNotes",
        ] {
            let leaked: Vec<&String> = bucket
                .keys()
                .filter(|key| key.starts_with(&format!("{group}:")))
                .collect();
            assert!(
                leaked.is_empty(),
                "{group}: must stay out of the bucket, found {leaked:?}"
            );
        }
    }

    /// The PNG text chunks are metadata and the PNG container's own image
    /// properties are not: ExifTool reports both in the same `PNG` group, and
    /// the app models width and height natively — a `PNG:ImageWidth` in the
    /// bucket would be a third copy of the same number.
    #[test]
    fn the_png_container_properties_are_not_metadata_but_its_text_chunks_are() {
        let record = json!({
            "PNG:ImageWidth": 16,
            "PNG:ImageHeight": 16,
            "PNG:BitDepth": 8,
            "PNG:ColorType": "Grayscale",
            "PNG:Compression": "Deflate/Inflate",
            "PNG:Filter": "Adaptive",
            "PNG:Interlace": "Noninterlaced",
            "PNG:BackgroundColor": 0,
            "PNG:SRGBRendering": "Perceptual",
            "PNG:Title": "The Title",
            "PNG:Description": "A Description",
            "PNG:Comment": "A Comment",
            "PNG:ZKeyword": "zlib text value"
        });
        let bucket = map_further_fields(&XmpSource::Unavailable, Some(&record));

        assert_eq!(
            bucket,
            BTreeMap::from([
                ("PNG:Comment".to_string(), "A Comment".to_string()),
                ("PNG:ZKeyword".to_string(), "zlib text value".to_string()),
            ]),
            "the two natively consumed chunks are excluded and the container's own \
             image properties are not metadata: {bucket:?}"
        );
    }

    /// The bucket follows the sidecar for the XMP family, exactly as the native
    /// fields do: with a sidecar present, the packet inside the image is masked,
    /// so a key of the image's own XMP packet must not reach the bucket while
    /// the image's IIM record still contributes to it.
    #[test]
    fn the_bucket_reads_xmp_from_the_sidecar_when_one_exists() {
        let sidecar = json!({
            "XMP-dc:Title": "Custom Title",
            "XMP-xmp:CreatorTool": "snapfab 1.0"
        });
        let image = recorded_iptc_only();
        let bucket = map_further_fields(&XmpSource::Record(&sidecar), Some(&image));

        assert_eq!(
            bucket.get("XMP-xmp:CreatorTool").map(String::as_str),
            Some("snapfab 1.0"),
            "the sidecar's XMP is the XMP source: {bucket:?}"
        );
        for masked in ["XMP-dc:Title", "XMP-dc:Description", "XMP-dc:Subject"] {
            assert!(
                !bucket.contains_key(masked),
                "{masked} is not in the sidecar, so the image's masked packet cannot \
                 contribute it: {bucket:?}"
            );
        }
        assert_eq!(
            bucket.get("IPTC:Headline").map(String::as_str),
            Some("IPTC Headline"),
            "the image's IIM record is not masked by the sidecar: {bucket:?}"
        );
    }

    /// Every value is a display string, as `exifVec`'s are: a JSON number, a
    /// boolean and a list all have to flatten into one string, because the
    /// bucket is `String -> String` and the sidebar prints it as text. The
    /// keys are deliberately ones the native mapping does not consume — a
    /// natively consumed key never reaches the bucket whatever shape its value
    /// has, which is [`no_native_key_is_repeated_in_the_bucket`]'s subject.
    #[test]
    fn bucket_values_are_flattened_to_display_strings() {
        let xmp = json!({
            "XMP-pdf:Producer": 4,
            "XMP-xmpRights:Marked": true,
            "XMP-photoshop:Credit": ["a", "b"]
        });
        let image = json!({ "IPTC:Urgency": 2, "IPTC:By-line": "" });
        let bucket = map_further_fields(&XmpSource::Record(&xmp), Some(&image));

        assert_eq!(
            bucket.get("XMP-pdf:Producer").map(String::as_str),
            Some("4"),
            "a JSON number is not a string: {bucket:?}"
        );
        assert_eq!(bucket.get("IPTC:Urgency").map(String::as_str), Some("2"));
        assert_eq!(
            bucket.get("XMP-xmpRights:Marked").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            bucket.get("XMP-photoshop:Credit").map(String::as_str),
            Some("a, b"),
            "a list is one entry per value, joined as `exifVec` joins one: {bucket:?}"
        );
        assert!(
            !bucket.contains_key("IPTC:By-line"),
            "a valueless tag is absent, not the empty string: {bucket:?}"
        );
    }

    /// The same rule for a value that is only whitespace, which is what an
    /// emptied `lang-alt` arrives as.
    #[test]
    fn a_blank_bucket_value_is_absent_rather_than_an_empty_row() {
        let image = json!({ "IPTC:By-line": "   ", "IPTC:City": "fixtureville" });
        let bucket = map_further_fields(&XmpSource::Unavailable, Some(&image));

        assert!(
            !bucket.contains_key("IPTC:By-line"),
            "a blank is not a value: {bucket:?}"
        );
        assert_eq!(
            bucket.get("IPTC:City").map(String::as_str),
            Some("fixtureville")
        );
    }

    /// No record, no bucket — the path a non-fallible caller lands on when
    /// `ExifTool` cannot read the file, and the shape the API reports as `{}`.
    #[test]
    fn no_record_yields_an_empty_bucket() {
        assert!(map_further_fields(&XmpSource::Unavailable, None).is_empty());
        // A record with nothing but the excluded groups is the same outcome.
        let record = json!({
            "File:FileType": "JPEG",
            "System:FileName": "bare.jpg",
            "IFD0:Make": "Canon"
        });
        assert!(map_further_fields(&XmpSource::Record(&record), Some(&record)).is_empty());
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

    /// The two keys a writing tool's own stamps contribute to the bucket, and
    /// the only ones a fixture gets without `further_iptc`.
    ///
    /// `IPTC:ApplicationRecordVersion` is IIM 2:00, the record version every
    /// writer of an IIM record sets, and `XMP-x:XMPToolkit` is the name the
    /// writing tool puts in the packet it serialises — the XMP counterpart of
    /// the EXIF `Software` tag. Both are metadata the app does not model, so
    /// `map_further_fields` files them here on purpose (see its doc), and both
    /// are the *writer's* identity rather than anything the fixture asked for.
    ///
    /// The expected set is read back out of the record rather than written out
    /// literally, because `XMP-x:XMPToolkit` carries the ExifTool version —
    /// 13.59 here, a different one in CI and in the Docker image — and a
    /// version is not a contract. The *keys* are, which is what the assertion
    /// therefore rests on, and the check at the end is what stops the
    /// derivation from going vacuous: a record that stopped carrying a stamp
    /// would quietly shrink the expectation instead of failing.
    fn writer_stamps(image: &serde_json::Value) -> BTreeMap<String, String> {
        use serde_json::Value;
        let Value::Object(entries) = image else {
            panic!("the record is an object, as `read_metadata_record` returns one")
        };
        let stamps: BTreeMap<String, String> = entries
            .iter()
            .filter(|(key, _)| {
                matches!(
                    key.as_str(),
                    "IPTC:ApplicationRecordVersion" | "XMP-x:XMPToolkit"
                )
            })
            .map(|(key, value)| {
                (
                    key.clone(),
                    super::json_value_to_string(value).expect("a stamp always has a value"),
                )
            })
            .collect();
        assert_eq!(
            stamps.keys().collect::<Vec<_>>(),
            vec!["IPTC:ApplicationRecordVersion", "XMP-x:XMPToolkit"],
            "the record must carry both writer stamps, or this expectation is vacuous: \
             {stamps:?}"
        );
        stamps
    }

    /// The bucket on a real file, through the real reader: the split holds
    /// against what `ExifTool` actually reports for a JPEG, not only against a
    /// recorded payload. The fixture is asked for the unmodelled IIM datasets
    /// (`further_iptc`), and the bucket must be exactly those three plus the
    /// writer's two stamps — the keywords, title and caption it shares with the
    /// native mapping are not in it, and neither is the EXIF family, the JFIF
    /// parameters, the container facts or the derived `Composite:` values.
    #[test]
    fn a_generated_jpeg_lands_only_its_unmodelled_datasets_and_writer_stamps_in_the_bucket() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = snapfab_further_jpeg(dir.path(), "further.jpg", &["bucket_keyword"]);

        let image = read_metadata_record(&photo).expect("read image");
        let data = asset_metadata_for(&photo, Some(&image));

        // Control: the natively consumed half of the same IIM record is filled.
        // snapfab adds the render mode's name to the keywords, so the set holds
        // the spec keyword plus one more whose name depends on the rng.
        assert!(
            data.native.tags.contains("bucket_keyword"),
            "control: the fixture's keyword must be indexed: {:?}",
            data.native.tags
        );
        assert!(data.native.description.is_some());
        assert!(data.native.title.is_some());

        let mut expected: BTreeMap<String, String> = BTreeMap::from([
            (
                "IPTC:By-line".to_string(),
                snapfab::FURTHER_IPTC_BY_LINE.to_string(),
            ),
            (
                "IPTC:City".to_string(),
                snapfab::FURTHER_IPTC_CITY.to_string(),
            ),
            (
                "IPTC:CopyrightNotice".to_string(),
                snapfab::FURTHER_IPTC_COPYRIGHT.to_string(),
            ),
        ]);
        expected.extend(writer_stamps(&image));

        assert_eq!(
            data.further, expected,
            "the bucket is the three unmodelled datasets plus the writer's stamps and \
             nothing else: {:?}",
            data.further
        );
    }

    /// The same JPEG without `further_iptc` has nothing but the writer's stamps
    /// in the bucket: every IIM dataset it writes is one the native mapping
    /// consumed. Without this the test above could not tell "the split works"
    /// from "snapfab wrote more" — the stamps are in both buckets, so they
    /// cannot be what distinguishes them.
    #[test]
    fn a_stock_generated_jpeg_buckets_only_the_writer_stamps() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = snapfab_jpeg(dir.path(), "stock.jpg", &["bucket_keyword"]);

        let image = read_metadata_record(&photo).expect("read image");
        let data = asset_metadata_for(&photo, Some(&image));

        assert!(
            !data.native.tags.is_empty(),
            "control: the fixture is tagged"
        );
        assert_eq!(
            data.further,
            writer_stamps(&image),
            "a fixture writing only Keywords/ObjectName/Caption has only the writer's own \
             stamps left to report: {:?}",
            data.further
        );
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
            further_iptc: None,
            minimal: false,
        }])
        .expect("generate photo");
        photo
    }

    /// A JPEG written by snapfab with `tags` and `further_iptc`, so its IIM
    /// record holds the three datasets the native mapping consumes *and* three
    /// it does not. This is the shape the API-level scenario
    /// `metadata_detail_exposes_further_data` drives.
    fn snapfab_further_jpeg(dir: &Path, name: &str, tags: &[&str]) -> std::path::PathBuf {
        let photo = dir.join(name);
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(photo.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: Some(tags.iter().map(|tag| (*tag).to_string()).collect()),
            exif_date: Some("2024:05:06 07:08:09".into()),
            further_iptc: Some(true),
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
            further_iptc: None,
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

    /// The premise of the numbered-group tests, through the real reader: a JPEG
    /// with two IIM records in one APP13 block really is reported as `IPTC:` plus
    /// `IPTC2:`, and the numbered record's caption and keywords reach the fields
    /// the standard record left empty. Before the mapping read all three group
    /// names, this file indexed with no caption and no tags at all — measured
    /// against the recorded payload
    /// `recorded_iptc_numbered_records` rather than assumed from it.
    #[test]
    fn a_jpeg_with_two_iim_records_reads_both_them() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = jpeg_with_two_iim_records(dir.path(), "two-records.jpg");

        let image = read_metadata_record(&photo).expect("read image");
        // Control: the engine really does number the second record's group. If
        // it stopped doing so, the assertions below would be measuring a
        // mapping that no longer has a case to cover.
        for key in [
            "IPTC:ObjectName",
            "IPTC2:Caption-Abstract",
            "IPTC2:Keywords",
        ] {
            assert!(
                image.get(key).is_some(),
                "{key} must be reported for this fixture, got: {}",
                image
            );
        }

        let data = asset_metadata_for(&photo, Some(&image));
        assert_eq!(
            data.native.title.as_deref(),
            Some("Standard record object name")
        );
        assert_eq!(
            data.native.description.as_deref(),
            Some("Numbered record caption"),
            "the caption only the numbered record carries: {:?}",
            data.native
        );
        assert_eq!(
            data.native.tags,
            HashSet::from([
                "standard_kw".to_string(),
                "numbered_kw".to_string(),
                "second_numbered_kw".to_string()
            ])
        );
        assert_eq!(
            data.further.get("IPTC2:City").map(String::as_str),
            Some("Numberedville"),
            "a dataset the mapping does not read still reaches the bucket, under the \
             group ExifTool filed it in: {:?}",
            data.further
        );
    }

    /// snapfab's JPEG with a second IIM record appended to its APP13 block.
    ///
    /// An APP13 marker segment holding a Photoshop 3.0 image-resource block,
    /// built field by field: the `8BIM` signature, the two-byte resource id
    /// `0x0404` (the IPTC-NAA record), an empty Pascal name padded to an even
    /// length, the four-byte resource size, and the IIM datasets themselves —
    /// each a `0x1c` marker, record number, dataset number, then a big-endian
    /// length. Two `0x0404` resources in one block is what makes ExifTool file
    /// the second one under a numbered group.
    fn jpeg_with_two_iim_records(dir: &Path, name: &str) -> std::path::PathBuf {
        let photo = snapfab_jpeg(dir, name, &[]);
        let mut bytes = std::fs::read(&photo).expect("read generated jpeg");

        let iim = |datasets: &[(u8, u8, &[u8])]| {
            let mut out = Vec::new();
            for (record, dataset, value) in datasets {
                out.push(0x1c);
                out.push(*record);
                out.push(*dataset);
                out.extend_from_slice(&(value.len() as u16).to_be_bytes());
                out.extend_from_slice(value);
            }
            out
        };
        let resource = |datasets: &[(u8, u8, &[u8])]| {
            let data = iim(datasets);
            let mut out = b"8BIM\x04\x04\x00\x00".to_vec();
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(&data);
            if data.len() % 2 == 1 {
                // Photoshop pads an odd-length resource to an even one.
                out.push(0);
            }
            out
        };
        let mut block = b"Photoshop 3.0\x00".to_vec();
        block.extend_from_slice(&resource(&[
            (2, 0, &[0x00, 0x04]),
            (2, 5, b"Standard record object name"),
            (2, 25, b"standard_kw"),
        ]));
        block.extend_from_slice(&resource(&[
            (2, 0, &[0x00, 0x04]),
            (2, 5, b"Numbered record object name"),
            (2, 25, b"numbered_kw"),
            (2, 25, b"second_numbered_kw"),
            (2, 90, b"Numberedville"),
            (2, 120, b"Numbered record caption"),
        ]));
        assert!(block.len() <= u16::MAX as usize, "APP13 payload too large");
        let segment = [
            vec![0xff, 0xed],
            ((block.len() + 2) as u16).to_be_bytes().to_vec(),
            block,
        ]
        .concat();

        // Inserted after SOI, which the file opens with.
        bytes.splice(2..2, segment);
        std::fs::write(&photo, &bytes).expect("write jpeg with two records");
        photo
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
