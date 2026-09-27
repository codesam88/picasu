use crate::model::abstract_data::AbstractData;
use anyhow::{Context, Result, anyhow};
use exiftool::{ExifTool, ExifToolError};
use regex::Regex;
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, path::Path, process::Command, sync::LazyLock};

/// The metadata engine for image EXIF. `ExifTool` is an external binary (pure
/// Perl, invoked as a separate process like `ffprobe` below), and there is no
/// in-process fallback: [`read_metadata_record`] is the only reader of image
/// metadata, so a missing binary empties every image's `exifVec`.
const EXIFTOOL: &str = "exiftool";

/// Date format asked of `ExifTool` with `-d`, applied to every date-valued tag.
///
/// Dash-separated on purpose: `AbstractData::compute_timestamp` parses
/// `DateTimeOriginal` with exactly `%Y-%m-%d %H:%M:%S`, and the stored map is
/// shown to users. `ExifTool`'s own default is `2024:05:06 07:08:09`, which
/// would silently push every DateTimeOriginal-sorted photo down to the next
/// sort priority. Date-only tags (`GPSDateStamp`) are not reformatted and keep
/// `ExifTool`'s `YYYY:MM:DD`.
const EXIFTOOL_DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// The one key `ExifTool` adds to every `-j` record that is not a tag: the path it
/// read. It is dropped, since an absolute path is not metadata.
const SOURCE_FILE_KEY: &str = "SourceFile";

/// Extra arguments appended to the `exiftool` crate's own `-json` on every read.
///
/// * `-G1` asks for family-1 group names as `Group:Tag` key prefixes. It is what
///   makes one read serve both consumers: [`exif_map_from_record`] keeps the
///   EXIF family, and the native-field mapping takes `XMP-*`, `IPTC` and `PNG`
///   from the same record instead of paying for a second `exiftool` call. The
///   uppercase spelling matters — lowercase `-g1` nests the groups as JSON
///   objects, which is not the shape this code reads.
/// * `-d` fixes the date shape, see [`EXIFTOOL_DATE_FORMAT`].
///
/// The crate forwards each entry as its own line of the `-stay_open` argument
/// file, so a value containing a space stays one argument.
const READ_ARGS: &[&str] = &["-G1", "-d", EXIFTOOL_DATE_FORMAT];

/// Everything `ExifTool` reports for one file, keyed by family-1 group name
/// (`IFD0`, `ExifIFD`, `XMP-dc`, `IPTC`, `PNG`, …) and then by tag name.
///
/// This is the whole-record shape the metadata engine works in. The
/// `exifVec` projection of it is [`exif_map_from`]; the native-field mapping
/// (XMP > IPTC > PNG-text precedence) reads the other groups out of the very
/// same call.
pub(crate) type GroupedMetadata = BTreeMap<String, BTreeMap<String, String>>;

/// Project one `ExifTool` record down to the `exifVec` map: the EXIF family
/// only, group prefixes stripped.
///
/// The contract of that map, which is what the API exposes as `exifVec`:
///
/// * **Keys** are `ExifTool`'s EXIF-family tag names, unprefixed. The read is
///   *not* narrowed to the EXIF family (that is what makes it one call for
///   everything, see [`READ_ARGS`]); instead [`exif_map_from`] keeps only the
///   EXIF-family groups — `IFD0`/`IFD1`/…, `ExifIFD`, `InteropIFD`, `GPS`,
///   `SubIFD` — and strips the group prefix, so `DateTimeOriginal`, `Make`,
///   `Model` and `Orientation` keep the names the previous in-process reader
///   used while XMP, IPTC, PNG-text, maker notes, container, file-system and
///   composite tags cannot leak in. Those belong to the native-field mapping
///   (`process::xmp`), not to the EXIF map.
/// * **Values** are `ExifTool`'s print-converted form, flattened to strings:
///   `1/3188` for `ExposureTime`, `Rotate 90 CW` for `Orientation`,
///   `Uncompressed` for `Compression`, `19.7` for a JSON number. This keeps the
///   map readable in the sidebar; `-n` would print raw numbers instead.
/// * **Dates** use [`EXIFTOOL_DATE_FORMAT`]. Two tag names move with the engine
///   swap: EXIF 0x0131 is `ModifyDate` and 0x9004 is `CreateDate` (the previous
///   reader called them `DateTime` and `DateTimeDigitized`), and TIFF 0x0101 is
///   `ImageHeight` rather than `ImageLength`.
///
/// A record that could not be read is the caller's to absorb: the indexer turns
/// a failed read into an empty map, which is what keeps a broken image from
/// turning into a rejected asset. The API-level counterpart is
/// `corrupt_exif_in_decodable_image_yields_empty_exif_vec`.
pub(crate) fn exif_map_from_record(record: &Value) -> BTreeMap<String, String> {
    exif_map_from(&group_by_family(record))
}

/// The raw `-json` record for one file, before any grouping or flattening.
///
/// One call per file, whatever the caller ends up using: the caller projects
/// the record it wants out of the result — [`exif_map_from_record`] for the
/// `exifVec` map, `process::xmp` for the native fields.
///
/// The record is the currency rather than the [`GroupedMetadata`] projection
/// because not every consumer wants that projection: the native-field mapping
/// needs `XMP-dc:Subject` and `IPTC:Keywords` as *lists*, and grouping flattens
/// a list into one `", "`-separated string, which would make a keyword
/// containing a comma impossible to recover. It is also what keeps the read
/// count at one per file: `exifVec` and the native fields are two projections
/// of the same record, and a caller holding it derives both instead of paying
/// for a second `ExifTool` call.
///
/// # Errors
///
/// `Err` means the read did not happen — no session could be started (a missing
/// or unrunnable binary, named in the context), the transport to the persistent
/// child broke, or the output was not a JSON record. A file `ExifTool` cannot
/// parse is *not* an error here: it comes back as a record with no EXIF groups,
/// which the callers render as an empty map.
pub(crate) fn read_metadata_record(file_path: &Path) -> Result<Value> {
    SESSION.with(|slot| -> Result<Value> {
        let mut slot = slot.borrow_mut();
        // A session that could not be started is never cached, so a missing
        // binary is reported per read instead of latching as a dead session.
        if slot.is_none() {
            *slot = Some(Session::start()?);
        }
        let session = slot.as_mut().expect("session installed just above");
        session
            .read_retrying(file_path)
            .with_context(|| format!("failed to read metadata for {}", file_path.display()))
    })
}

thread_local! {
    /// One `exiftool -stay_open` session per calling thread.
    ///
    /// *Stay-open, not per call*: a cold `exiftool` spends ~170 ms in Perl
    /// startup against ~12 ms of actual work, so the session is what makes the
    /// engine affordable at all. It is `thread_local` rather than one shared
    /// session because the `exiftool` crate serialises a session's requests
    /// behind its own mutex, which would cap the server at one read at a time
    /// however many index workers are free. The cost of `thread_local` is a
    /// child per calling thread, which stays bounded because the only caller
    /// runs on the fixed-size `WORKER_RAYON_POOL`; a thread that reaches the
    /// reader some other way pays one startup and then keeps its own child for
    /// as long as it lives. `measures_the_metadata_read_cost` measures both
    /// shapes.
    ///
    /// The session is dropped, and its child killed, when the owning thread
    /// exits; there is no cross-thread sharing and no global state to reset
    /// between tests.
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

/// A running `exiftool -stay_open` child plus the one read that has to survive
/// its death.
struct Session {
    tool: ExifTool,
}

impl Session {
    /// Spawn the persistent child, resolving [`EXIFTOOL`] on `PATH`.
    fn start() -> Result<Self> {
        Self::start_at(Path::new(EXIFTOOL))
    }

    /// Spawn the persistent child from `executable`, attaching the install
    /// remedy to a failure.
    ///
    /// A binary that is missing or not runnable is reported as
    /// `ExifToolNotFound` carrying the OS error underneath, which is what makes
    /// the cause nameable in [`start_context`]; taking the path as an argument
    /// rather than hardcoding it is what lets a test observe that error without
    /// taking `exiftool` off the process-wide `PATH` for every other test.
    fn start_at(executable: &Path) -> Result<Self> {
        ExifTool::with_executable(executable)
            .map(|tool| Session { tool })
            .with_context(start_context)
    }

    fn read(&self, file_path: &Path) -> Result<Value, ExifToolError> {
        self.tool.json(file_path, READ_ARGS)
    }

    /// Read through the session, restarting the child at most once.
    ///
    /// A transport failure means the child died, not that the file is
    /// unreadable: the pipe is broken, or the child exited and the read hit
    /// end-of-stream. Both leave the session permanently unusable, so it is
    /// replaced and the read retried once. Everything else — a file
    /// `ExifTool` rejects, unparsable output — is reported as it is, since
    /// restarting would only pay the Perl startup cost to reach the same
    /// answer.
    fn read_retrying(&mut self, file_path: &Path) -> Result<Value> {
        match self.read(file_path) {
            Err(err) if is_transport_failure(&err) => {
                *self = Session::start()?;
                Ok(self.read(file_path)?)
            }
            other => Ok(other?),
        }
    }
}

/// Whether `err` says the persistent child is gone rather than the file being
/// unreadable, and so whether retrying on a fresh child can help.
fn is_transport_failure(err: &ExifToolError) -> bool {
    match err {
        // Every way the crate can say the child is gone rather than the file
        // being unreadable: writing to or reading from a dead child's pipes, a
        // stderr reader that ended with the child, and the documented
        // `ProcessTerminated` — which the crate never actually constructs, so the
        // end-of-stream error it reports instead is matched on its message too.
        ExifToolError::Io(_)
        | ExifToolError::StderrDisconnected
        | ExifToolError::ProcessTerminated => true,
        ExifToolError::ExifToolProcess { message, .. } => {
            message.contains("terminated unexpectedly")
        }
        _ => false,
    }
}

/// The install remedy [`read_all_metadata`] attaches to a session that could not
/// be started. Without a working `exiftool` the reader is non-fallible, so the
/// failure surfaces as an empty `exifVec` on every image rather than as an
/// error, and the only place the cause is visible is this message.
fn start_context() -> String {
    format!(
        "failed to start {EXIFTOOL} in -stay_open mode; install it with \
         `just install-exiftool` (or `apt-get install libimage-exiftool-perl`)"
    )
}

/// Split one `-json` record into [`GroupedMetadata`], dropping the echoed path
/// and every entry `ExifTool` reported as having no value.
fn group_by_family(record: &Value) -> GroupedMetadata {
    let Value::Object(entries) = record else {
        return GroupedMetadata::new();
    };

    let mut grouped = GroupedMetadata::new();
    for (key, value) in entries {
        if key == SOURCE_FILE_KEY {
            continue;
        }
        // `-G1` prefixes every tag with its group; an unprefixed key is a
        // record-level field rather than a tag and has no group to file it under.
        let Some((group, tag)) = key.split_once(':') else {
            continue;
        };
        let Some(text) = json_value_to_string(value) else {
            continue;
        };
        grouped
            .entry(group.to_string())
            .or_default()
            .insert(tag.to_string(), text);
    }
    grouped
}

/// Family-1 groups that belong to the EXIF family, in resolution order.
///
/// These are the directory names `ExifTool` reports for EXIF IFDs under `-G1`:
/// `IFD0` is the primary directory, `IFD1` the thumbnail, `ExifIFD` the EXIF
/// sub-directory, and so on. `-EXIF:all` selected the same tags by tag table,
/// and the two agree on every JPEG, PNG, TIFF and WebP in `ExifTool`'s own
/// 194-file test corpus. Everything else a `-G1` read returns is a different
/// family and belongs to another consumer: `XMP-*` and `IPTC` to the
/// native-field mapping, `PNG` to the text-chunk mapping, `MakerNotes` to a
/// per-vendor reader, `File`/`System`/`Composite` to nobody.
///
/// The numbered forms are matched by shape rather than enumerated, because
/// `SubIFD7` or `IFD4` are not hypothetical: a multi-page TIFF or a raw file
/// with nested previews numbers its directories past the first two.
const EXIF_FAMILY_GROUPS: &[&str] = &["IFD0", "IFD1", "IFD2", "IFD3"];

/// Position of an EXIF-family group in the order [`exif_map_from`] resolves a
/// duplicated tag name in. Lower wins.
type GroupRank = (usize, usize);

/// `Some(rank)` when `group` is an EXIF-family directory name, ordered by
/// [`EXIF_FAMILY_GROUPS`] first and the numbered variants after them.
///
/// Lower rank wins when the same tag name appears in two IFDs — see
/// [`exif_map_from`].
fn exif_group_rank(group: &str) -> Option<GroupRank> {
    if let Some(at) = EXIF_FAMILY_GROUPS.iter().position(|known| *known == group) {
        return Some((0, at));
    }
    let numbered = |prefix: &str, digits_required: bool| {
        let rest = group.strip_prefix(prefix)?;
        (!digits_required || !rest.is_empty())
            .then_some(rest)
            .filter(|rest| rest.bytes().all(|byte| byte.is_ascii_digit()))
    };
    // IFD4 and beyond: a multi-page TIFF's later directories. Ranked after the
    // named ones so a document's own metadata beats a later page's.
    if numbered("IFD", true).is_some() {
        return Some((1, 0));
    }
    if group == "ExifIFD" || group == "InteropIFD" || group == "GPS" {
        return Some((2, 0));
    }
    // SubIFD, SubIFD1, …: the preview and thumbnail directories a raw file
    // nests under IFD0, ranked last so they cannot displace the real image.
    if numbered("SubIFD", false).is_some() {
        return Some((3, 0));
    }
    None
}

/// Project [`GroupedMetadata`] down to the `exifVec` map: the EXIF family only,
/// group prefixes stripped.
///
/// A tag name can occur in more than one IFD (`ImageWidth` exists in `IFD0` and
/// again in the `IFD1` thumbnail of the same file). The lowest-ranked group wins
/// — the primary directory over the thumbnail, the thumbnail over a nested
/// preview — so the map describes the image the user is looking at.
///
/// `ExifTool`'s own ungrouped output resolves the same collision differently on
/// a file with an embedded preview: it follows whichever directory holds the
/// full-resolution data, which for a raw file is a `SubIFD`, not `IFD0`. The
/// difference is confined to raw and multi-directory formats — picasu indexes
/// JPEG and PNG — and the two agree everywhere else;
/// `the_grouped_read_matches_the_ungrouped_exif_read_byte_for_byte` holds the
/// shipped fixtures to that.
fn exif_map_from(grouped: &GroupedMetadata) -> BTreeMap<String, String> {
    // First pass: the best rank any EXIF-family group offers for each tag name.
    // Rank order rather than map order, so the winner does not depend on how
    // `serde_json` happens to order the record's keys.
    let mut best_rank: BTreeMap<&str, GroupRank> = BTreeMap::new();
    for (group, tags) in grouped {
        let Some(rank) = exif_group_rank(group) else {
            continue;
        };
        for tag in tags.keys() {
            best_rank
                .entry(tag.as_str())
                .and_modify(|held| *held = (*held).min(rank))
                .or_insert(rank);
        }
    }

    // Second pass: the values from the groups that hold that best rank.
    let mut exif = BTreeMap::new();
    for (group, tags) in grouped {
        let Some(rank) = exif_group_rank(group) else {
            continue;
        };
        for (tag, value) in tags {
            if best_rank[tag.as_str()] == rank {
                exif.insert(tag.clone(), value.clone());
            }
        }
    }
    exif
}

/// Flatten one `ExifTool` JSON value into the single string the map holds.
///
/// `ExifTool`'s JSON is not uniformly typed: `ExposureTime` arrives as the string
/// `"1/3188"`, `FNumber` as the number `19.7`, a multi-valued tag as an array.
/// `null` means "no value" and is dropped rather than stored as the text
/// `"null"`; an object cannot occur while `-struct` is not requested, and is
/// kept as JSON so nothing is silently lost if that ever changes.
fn json_value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(json_value_to_string)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        Value::Null => None,
        Value::Object(_) => Some(value.to_string()),
    }
}

static RE_VIDEO_INFO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(.*?)=(.*?)\n").expect("regex compilation failure"));

/// Use `ffprobe` to retrieve metadata for videos, propagating every error
/// with rich context strings.
pub fn generate_exif_for_video(abstract_data: &AbstractData) -> Result<BTreeMap<String, String>> {
    let source_path = abstract_data.source_path_string();
    let mut exif_tuple = BTreeMap::new();

    // Spawn ffprobe and capture its output
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_format")
        .arg("-show_streams")
        .arg(source_path)
        .output()
        .context(format!("failed to spawn ffprobe for {source_path}"))?;

    if output.status.success() {
        // Convert raw bytes to UTF‑8 text
        let stdout = String::from_utf8(output.stdout).context(format!(
            "failed to convert ffprobe stdout to UTF‑8 for {source_path}"
        ))?;

        // Regex‑parse key/value pairs
        for cap in RE_VIDEO_INFO.captures_iter(&stdout) {
            let key = cap
                .get(1)
                .context(format!("capture group 1 missing in {source_path}"))?
                .as_str()
                .to_string();
            let value = cap
                .get(2)
                .context(format!("capture group 2 missing in {source_path}"))?
                .as_str()
                .to_string();
            exif_tuple.insert(key, value);
        }

        Ok(exif_tuple)
    } else {
        Err(anyhow!(
            "ffprobe exited with status {:?} for {}",
            output.status.code().unwrap_or(-1),
            source_path
        ))
    }
}

#[cfg(test)]
use std::path::PathBuf;

/// Locate `tool` as an executable file on `PATH`.
///
/// Mirrors `process::video`'s copy, including its two-level check: the result
/// is a claim that the file exists and carries the execute bit, which
/// [`tool_runs`] then confirms by running the binary. The duplication is
/// deliberate for this change — `video.rs` keeps its helpers private and
/// test-only, so sharing them means a module both files can see, which is a
/// refactor of `video.rs` that belongs to whoever next touches it.
#[cfg(test)]
fn resolve_on_path(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(tool))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(test)]
fn is_executable_file(candidate: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(candidate)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Run `path -ver` and report whether the binary is usable. ExifTool's version
/// flag is `-ver`; the video tools use `-version`.
#[cfg(test)]
fn tool_runs(path: &std::path::Path) -> bool {
    std::process::Command::new(path)
        .arg("-ver")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Check that the ExifTool toolchain is present, or explain what is missing.
///
/// Deliberately a hard failure rather than a silent skip: `generate_exif_for_image`
/// is non-fallible, so a missing ExifTool turns every image's `exifVec` into
/// `{}` and the suite goes green on assertions it never really checked.
/// `resolved` is a slice of `(tool, path-on-PATH)` pairs so the message can be
/// tested without removing a binary from the environment.
#[cfg(test)]
fn check_exiftool_toolchain(resolved: &[(&str, Option<PathBuf>)]) -> Result<(), String> {
    let missing = resolved
        .iter()
        .filter(|(_, path)| path.is_none())
        .map(|(tool, _)| *tool)
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }

    let named = missing
        .iter()
        .map(|tool| format!("`{tool}`"))
        .collect::<Vec<_>>()
        .join(" and ");
    Err(format!(
        "image metadata needs ExifTool on PATH, but {named} could not be found.\n\
         \n\
         This is an external binary, not an optional extra: `exif.rs::read_metadata_record` \
         is the only reader of image metadata and it holds an `exiftool -stay_open` session open to \
         read the whole metadata map, exactly the way the video path asks `ffprobe`. Without it \
         no image EXIF can be read at all, so every `exifVec` would be empty and the scenarios \
         asserting an EXIF contract (`metadata_detail_returns_full_metadata`, the `png_`, `tiff_` \
         and `webp_metadata_exif_dimensions_thumbnail` scenarios) would pass vacuously or fail \
         on an empty map that hides the real cause.\n\
         \n\
         Install ExifTool to fix this (Debian/Ubuntu: `apt-get install libimage-exiftool-perl`; \
         `just install-exiftool` fetches the pinned pure-Perl distribution into \
         `~/.local` without root; the picasu runtime Docker image already does) and re-run \
         `cargo test -p picasu`."
    ))
}

/// The tools the image metadata path needs on `PATH`, resolved as
/// `(name, path)` pairs.
///
/// The name comes from [`EXIFTOOL`], the constant the reader itself spawns, so
/// the precondition and the code under test cannot drift apart: renaming the
/// binary in production code breaks this test instead of silently skipping it.
#[cfg(test)]
fn resolve_exiftool_tools() -> Vec<(&'static str, Option<PathBuf>)> {
    vec![(EXIFTOOL, resolve_on_path(EXIFTOOL))]
}

#[cfg(test)]
mod tests {
    use super::{
        GroupedMetadata, PathBuf, Session, check_exiftool_toolchain, exif_group_rank,
        exif_map_from, exif_map_from_record, group_by_family, is_transport_failure,
        json_value_to_string, read_metadata_record, resolve_exiftool_tools, tool_runs,
    };
    use exiftool::ExifToolError;
    use serde_json::json;
    use std::{
        collections::BTreeMap,
        path::Path,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    /// The `exifVec` projection of `path` with the non-fallible contract the
    /// indexer relies on: a read that fails is an empty map, never an error.
    /// The projection is reached from the record the indexer already read, so a
    /// path is all a test needs to state.
    fn exif_vec_of(path: &Path) -> BTreeMap<String, String> {
        read_metadata_record(path)
            .map(|record| exif_map_from_record(&record))
            .unwrap_or_default()
    }

    /// The documented non-fallible contract: a failure to read EXIF is not an
    /// error, it is an empty map. Every input here is a way the reader can fail,
    /// and none of them may panic or produce a partial map.
    ///
    /// The consequence is the one worth stating: a file with a damaged EXIF block
    /// is served with no EXIF at all rather than as a rejected asset. The API
    /// level of the same behaviour is
    /// `corrupt_exif_in_decodable_image_yields_empty_exif_vec`.
    #[test]
    fn an_unreadable_exif_block_yields_an_empty_map() {
        let dir = tempfile::tempdir().expect("temp dir");

        // A file that exists but is not an image at all.
        let not_an_image = dir.path().join("notes.jpg");
        std::fs::write(&not_an_image, b"this is not an image at all").expect("write file");
        assert!(
            exif_vec_of(&not_an_image).is_empty(),
            "a non-image must yield no EXIF"
        );

        // A JPEG header followed by nothing: the reader finds no EXIF segment.
        let stub = dir.path().join("stub.jpg");
        std::fs::write(&stub, [0xff, 0xd8, 0xff, 0xe0]).expect("write file");
        assert!(exif_vec_of(&stub).is_empty());

        // A path that does not exist: ExifTool reports it cannot read the file
        // and exits non-zero, which the reader turns into an empty map.
        let missing = dir.path().join("absent.jpg");
        assert!(exif_vec_of(&missing).is_empty());

        // An empty path, which is what a record with no `path` set produces.
        assert!(exif_vec_of(Path::new("")).is_empty());
    }

    /// A JPEG whose EXIF block cannot be parsed yields an empty map while the
    /// rest of the file stays a decodable image. The TIFF byte-order marker that
    /// follows the `Exif\0\0` header is corrupted in place, so the segment
    /// length and everything after it are untouched.
    ///
    /// The damage has to be one ExifTool cannot work around. It recovers from a
    /// damaged *IFD offset* — patching the last byte of `II*\0` (measured with
    /// exiftool 13.59) still yields every tag, because the directory is found by
    /// scanning — so a corrupted offset would fill this map rather than empty
    /// it. An invalid byte-order marker has no such fallback: ExifTool reports
    /// `Malformed APP1 EXIF segment` and extracts nothing.
    ///
    /// The unpatched control is asserted first, so a fixture that carries no
    /// EXIF in the first place cannot make this test pass for the wrong reason.
    #[test]
    fn a_corrupt_exif_block_yields_an_empty_map_while_the_image_stays_valid() {
        const TIFF_HEADER: &[u8] = b"Exif\0\0II*\0";
        const CORRUPT_TIFF_HEADER: &[u8] = b"Exif\0\0XY*\0";

        let dir = tempfile::tempdir().expect("temp dir");
        let good = dir.path().join("good.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(good.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: Some(vec!["exif_fallback_control".into()]),
            exif_date: Some("2023:07:15 10:00:00".into()),
            minimal: false,
        }])
        .expect("generate photo");

        // Control: the fixture really does carry a readable EXIF block.
        let control = exif_vec_of(&good);
        assert!(
            control.contains_key("Software"),
            "fixture carries no EXIF, so the negative case below would be vacuous: {control:?}"
        );

        let corrupt = dir.path().join("corrupt.jpg");
        let mut bytes = std::fs::read(&good).expect("read photo");
        let at = bytes
            .windows(TIFF_HEADER.len())
            .position(|window| window == TIFF_HEADER)
            .expect("fixture carries a little-endian Exif header");
        bytes[at..at + TIFF_HEADER.len()].copy_from_slice(CORRUPT_TIFF_HEADER);
        std::fs::write(&corrupt, &bytes).expect("write photo");

        assert_eq!(
            bytes.len() as u64,
            std::fs::metadata(&good).expect("stat").len(),
            "the patch must not change the file length"
        );
        assert!(
            exif_vec_of(&corrupt).is_empty(),
            "a corrupt EXIF block must yield no fields, not a partial map"
        );
    }

    /// The precondition for every EXIF scenario, and the counterpart of
    /// `process::video`'s ffmpeg check. It has to be a hard failure: the reader
    /// is non-fallible, so a missing ExifTool empties every `exifVec` and the
    /// suite reports success on assertions that were never really exercised.
    #[test]
    fn image_metadata_requires_a_working_exiftool() {
        let resolved = resolve_exiftool_tools();

        if let Err(diagnostic) = check_exiftool_toolchain(&resolved) {
            panic!("{diagnostic}");
        }

        for (tool, path) in &resolved {
            let path = path
                .as_ref()
                .expect("checked by check_exiftool_toolchain above");
            assert!(
                tool_runs(path),
                "`{tool}` resolved to {} on PATH but does not run (`{tool} -ver` failed); \
                 the image metadata path would fall back to an empty map on every image",
                path.display()
            );
        }
    }

    /// A complete toolchain is not an error, and a missing one names the tool
    /// rather than restating the list. The diagnostic has to say what breaks and
    /// how to fix it on all three supported environments, or it is only a
    /// prettier version of the failure.
    #[test]
    fn the_exiftool_diagnostic_names_only_the_missing_tool() {
        let complete = [("exiftool", Some(PathBuf::from("/usr/bin/exiftool")))];
        assert_eq!(check_exiftool_toolchain(&complete), Ok(()));

        let missing = check_exiftool_toolchain(&[("exiftool", None)])
            .expect_err("a missing exiftool must be reported");
        assert!(
            missing.contains("`exiftool` could not be found"),
            "the diagnostic must name the missing tool: {missing}"
        );
        for expected in [
            "no image EXIF can be read",
            "apt-get install libimage-exiftool-perl",
            "just install-exiftool",
            "cargo test -p picasu",
        ] {
            assert!(
                missing.contains(expected),
                "the diagnostic should mention {expected:?}: {missing}"
            );
        }
    }

    /// The map the API exposes as `exifVec`, pinned on a real file with a
    /// complete EXIF block: ExifTool's tag names, its print-converted values,
    /// and its number-to-string flattening.
    ///
    /// The pinned TIFF fixture is the right subject because every field in it is
    /// fixed in the bytes, so the assertions are about the reader's contract and
    /// not about a generator's random choices. It also carries the three
    /// renames an engine swap forces, which is why the *names* are as much the
    /// subject as the values:
    ///
    /// | EXIF tag          | in-process reader (kamadak) | ExifTool        |
    /// |-------------------|-----------------------------|-----------------|
    /// | 0x0131            | `DateTime`                  | `ModifyDate`    |
    /// | 0x9004            | `DateTimeDigitized`         | `CreateDate`    |
    /// | 0x0101 (height)   | `ImageLength`               | `ImageHeight`   |
    ///
    /// Only the *names* move: the values stay human-readable, which is why
    /// `XResolution` is `72` and not `72 pixels per inch`.
    #[test]
    fn exif_vec_carries_exiftool_tag_names_and_printed_values() {
        let exif = exif_vec_of(&pinned_fixture("tiff-48x32-exif"));

        for (key, value) in [
            // 0x0131/0x9003/0x9004: the three date tags, in the dash-separated
            // shape `-d %Y-%m-%d %H:%M:%S` asks for.
            ("ModifyDate", "2024-05-06 07:08:09"),
            ("DateTimeOriginal", "2024-05-06 07:08:09"),
            ("CreateDate", "2024-05-06 07:08:09"),
            // Dimensions and resolution: bare numbers, no unit suffix.
            ("ImageWidth", "48"),
            ("ImageHeight", "32"),
            ("XResolution", "72"),
            ("ResolutionUnit", "inches"),
            // List-valued and enum-valued tags, print-converted.
            ("BitsPerSample", "8 8 8"),
            ("Compression", "Uncompressed"),
            ("PhotometricInterpretation", "RGB"),
        ] {
            assert_eq!(
                exif.get(key).map(String::as_str),
                Some(value),
                "exifVec[{key:?}] should be the value ExifTool prints"
            );
        }

        // The names the old reader used are the ones that must be gone: a
        // leftover `DateTime` would mean the map mixes two vocabularies.
        for stale in ["DateTime", "DateTimeDigitized", "ImageLength"] {
            assert!(
                !exif.contains_key(stale),
                "exifVec still carries the retired key {stale:?}: {exif:?}"
            );
        }

        // `SourceFile` is ExifTool's echo of the path we just read. It is not a
        // tag, and an absolute path has no place in a map the API hands out.
        assert!(
            !exif.contains_key("SourceFile"),
            "the read path must not be stored as metadata: {exif:?}"
        );
    }

    /// A generated JPEG proves the same contract on the format the app indexes
    /// most, and pins the two boundaries of the read: the date shape
    /// `compute_timestamp` parses, and the fact that a JPEG carrying XMP and
    /// IPTC packets still reports only EXIF-family tags.
    ///
    /// `tags` makes snapfab write both an XMP packet and an APP13 IPTC block, so
    /// the negative assertions below are about real data in the file rather than
    /// about a file that happens to be bare.
    #[test]
    fn a_generated_jpeg_reports_its_capture_date_and_nothing_outside_exif() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = dir.path().join("tagged.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(photo.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: Some(vec![
                "engine_boundary_alpine".into(),
                "engine_boundary_winter".into(),
            ]),
            exif_date: Some("2024:05:06 07:08:09".into()),
            minimal: false,
        }])
        .expect("generate photo");

        let exif = exif_vec_of(&photo);
        assert_eq!(
            exif.get("DateTimeOriginal").map(String::as_str),
            Some("2024-05-06 07:08:09"),
            "`compute_timestamp` parses this value with `%Y-%m-%d %H:%M:%S`, so the \
             dash-separated shape is part of the contract, not a cosmetic choice: {exif:?}"
        );
        assert!(
            exif.contains_key("Software"),
            "control: the fixture carries no EXIF, so the assertions above are vacuous: {exif:?}"
        );
        for foreign in ["Subject", "Keywords", "Title", "Description", "FileName"] {
            assert!(
                !exif.contains_key(foreign),
                "exifVec must hold the EXIF family only; {foreign:?} belongs to the \
                 XMP/IPTC mapping: {exif:?}"
            );
        }
    }

    /// ExifTool's JSON is not uniformly typed, and the map is `String -> String`,
    /// so every value has to be flattened. The cases are pinned here rather than
    /// through a file because the interesting shapes (a list, a float, a `null`)
    /// do not all occur in the fixtures the suite ships.
    #[test]
    fn json_values_are_flattened_to_display_strings() {
        // Print-converted values arrive as strings and pass through untouched.
        assert_eq!(
            json_value_to_string(&json!("Rotate 90 CW")).as_deref(),
            Some("Rotate 90 CW")
        );
        // Integers and floats are numbers in the JSON, not strings.
        assert_eq!(json_value_to_string(&json!(5764)).as_deref(), Some("5764"));
        assert_eq!(json_value_to_string(&json!(19.7)).as_deref(), Some("19.7"));
        assert_eq!(json_value_to_string(&json!(true)).as_deref(), Some("true"));
        // A list-valued tag (IPTC keywords, XMP subject) is one entry per value.
        assert_eq!(
            json_value_to_string(&json!(["alpine", "winter"])).as_deref(),
            Some("alpine, winter")
        );
        // `null` means "no value" and must not be stored as the text "null".
        assert_eq!(json_value_to_string(&json!(null)), None);
        // `-struct` is not requested, so an object cannot occur; if one ever
        // does, keeping it as JSON loses nothing.
        assert_eq!(
            json_value_to_string(&json!({"a": 1})).as_deref(),
            Some("{\"a\":1}")
        );
    }

    /// Measure what the session bought, on this machine, against the cold spawn
    /// it replaced.
    ///
    /// `#[ignore]`d because it is a measurement, not a check: it prints
    /// percentiles and throughput and passes whatever the machine does. Run it
    /// with `cargo test -p picasu --lib -- --ignored --nocapture
    /// measures_the_metadata_read_cost` after changing the engine, the session
    /// policy, or the arguments — the number is the only evidence for the
    /// execution-path decision, and it goes stale silently otherwise.
    ///
    /// Three things are measured, all on the same pinned fixture and all through
    /// the production read path:
    ///
    /// * cold `exiftool -j` spawns, the invocation the crate replaced;
    /// * per-file cost through a live session, which is the number that decides
    ///   whether the engine is affordable at all;
    /// * aggregate throughput with the session shared under a mutex across N
    ///   threads versus one session per thread, which is the number that decides
    ///   where the session lives.
    #[test]
    #[ignore = "prints timings on this machine; not an assertion"]
    fn measures_the_metadata_read_cost() {
        const CALLS: usize = 120;
        let threads = rayon::current_num_threads();

        let path = pinned_fixture("tiff-48x32-exif");
        let path = path.as_path();
        let control = exif_vec_of(path);
        assert!(
            !control.is_empty(),
            "control: the fixture must read as EXIF before anything is timed"
        );

        let cold = time_calls(CALLS, || {
            let _ = ungrouped_exif_read(&path);
        });

        let session = Session::start().expect("start a session");
        let warmup = Instant::now();
        for _ in 0..10 {
            session.read(&path).expect("warm the session");
        }
        let warmup = warmup.elapsed();
        let per_file = time_calls(CALLS, || {
            let _ = session.read(&path).expect("read through the session");
        });

        // One session shared by THREADS threads against one session each.
        let shared = Arc::new(Mutex::new(Session::start().expect("start a session")));
        let shared_calls = 480usize;
        let shared_elapsed = Instant::now();
        std::thread::scope(|scope| {
            for _ in 0..threads {
                let shared = Arc::clone(&shared);
                scope.spawn(move || {
                    for _ in 0..shared_calls / threads {
                        let _ = shared
                            .lock()
                            .expect("session mutex")
                            .read(&path)
                            .expect("read through the shared session");
                    }
                });
            }
        });
        let shared_elapsed = shared_elapsed.elapsed();

        let per_thread_calls = 480usize;
        let per_thread_elapsed = Instant::now();
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(move || {
                    let own = Session::start().expect("start a session");
                    for _ in 0..per_thread_calls / threads {
                        let _ = own.read(&path).expect("read through its own session");
                    }
                });
            }
        });
        let per_thread_elapsed = per_thread_elapsed.elapsed();

        report("cold `exiftool -j` spawn", &cold);
        report("session, first call after start", &[warmup]);
        report("session, per-file", &per_file);
        println!(
            "  {:<34} {:>8.1} files/s aggregate over {threads} threads",
            "one session behind a mutex",
            shared_calls as f64 / shared_elapsed.as_secs_f64()
        );
        println!(
            "  {:<34} {:>8.1} files/s aggregate over {threads} threads",
            "one session per thread",
            per_thread_calls as f64 / per_thread_elapsed.as_secs_f64()
        );
    }

    /// Per-call durations of `calls` runs of `body`, excluding the first
    /// (which pays session and cache warm-up).
    fn time_calls(calls: usize, mut body: impl FnMut()) -> Vec<Duration> {
        body();
        (0..calls)
            .map(|_| {
                let start = Instant::now();
                body();
                start.elapsed()
            })
            .collect()
    }

    /// Print p50/p95/mean of a set of durations, in milliseconds.
    fn report(label: &str, durations: &[Duration]) {
        let mut sorted = durations.to_vec();
        sorted.sort_unstable();
        let at = |q: f64| {
            let index = ((sorted.len() - 1) as f64 * q).round() as usize;
            sorted[index].as_secs_f64() * 1000.0
        };
        let mean =
            sorted.iter().map(Duration::as_secs_f64).sum::<f64>() / sorted.len() as f64 * 1000.0;
        println!(
            "  {label:<34} p50 {:>7.2} ms  p95 {:>7.2} ms  mean {:>7.2} ms  (n={})",
            at(0.5),
            at(0.95),
            mean,
            sorted.len()
        );
    }

    /// Absolute path of a fixture pinned in the capability manifest, so the test
    /// reads the same bytes the manifest's SHA-256 check covers and the
    /// scenarios copy.
    fn pinned_fixture(id: &str) -> PathBuf {
        let entry = snapfab::capabilities::capabilities()
            .fixture_by_id(id)
            .unwrap_or_else(|| panic!("fixture `{id}` should be registered in the manifest"));
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the backend manifest dir has a parent: the repository root")
            .join(&entry.path)
    }

    /// Which family-1 group names count as EXIF.
    ///
    /// The read is not narrowed to the EXIF family — it is one `-G1` read for
    /// everything — so this predicate is the only thing standing between the
    /// `exifVec` map and every other metadata family ExifTool can report. Both
    /// halves matter: a name wrongly accepted leaks another family's tags into
    /// `exifVec`, and a name wrongly rejected drops a real EXIF tag.
    ///
    /// The accepted list is not invented: it is the set of family-1 group names
    /// that `exiftool -j -G1 -EXIF:all` reports for ExifTool's own test corpus
    /// (194 files with maker notes, GPS, multi-directory TIFFs and raw formats),
    /// which the parity test below then checks against real reads.
    #[test]
    fn exif_family_groups_are_recognised_and_nothing_else_is() {
        for group in [
            "IFD0",
            "IFD1",
            "IFD2",
            "IFD3",
            "IFD4",
            "ExifIFD",
            "InteropIFD",
            "GPS",
            "SubIFD",
            "SubIFD1",
            "SubIFD2",
        ] {
            assert!(
                exif_group_rank(group).is_some(),
                "{group:?} is an EXIF IFD directory and must be part of exifVec"
            );
        }

        for group in [
            // The other families, each with a real owner: the native-field
            // mapping, the PNG text-chunk mapping, per-vendor maker notes, and
            // nothing at all.
            "XMP",
            "XMP-dc",
            "XMP-xmp",
            "IPTC",
            "IPTC2",
            "PNG",
            "Photoshop",
            "ICC_Profile",
            "MakerNotes",
            "Canon",
            "Nikon",
            "File",
            "System",
            "Composite",
            "ExifTool",
            "RIFF",
            "JFIF",
            "SubIFDx",
            "PreviewIFD",
            "MetaIFD",
            "MPF0",
            "MIE-Main",
        ] {
            assert!(
                exif_group_rank(group).is_none(),
                "{group:?} is not an EXIF IFD directory and must not reach exifVec"
            );
        }

        // `IFD` without a number is a group name shape no ExifTool version
        // emits; accepting it would take a name that merely looks like an IFD.
        assert!(exif_group_rank("IFD").is_none());
    }

    /// The primary directory has to win a tag name that appears in two of them.
    ///
    /// Every multi-directory file has one: `ImageWidth` is in `IFD0` and again
    /// in the `IFD1` thumbnail, and raw files repeat the whole set under
    /// `SubIFD`. `exifVec` is `String -> String`, so exactly one value can
    /// survive, and the one that describes the image is the primary directory's.
    /// The reverse order would report a 160x106 thumbnail's dimensions for a
    /// 3040x2014 photo.
    #[test]
    fn a_duplicate_tag_name_resolves_to_the_primary_directory() {
        let mut grouped = GroupedMetadata::new();
        grouped.insert(
            "IFD1".to_string(),
            BTreeMap::from([
                ("ImageWidth".to_string(), "160".to_string()),
                (
                    "SubfileType".to_string(),
                    "Reduced-resolution image".to_string(),
                ),
            ]),
        );
        grouped.insert(
            "SubIFD1".to_string(),
            BTreeMap::from([("ImageWidth".to_string(), "3040".to_string())]),
        );
        grouped.insert(
            "IFD0".to_string(),
            BTreeMap::from([
                ("ImageWidth".to_string(), "3040".to_string()),
                (
                    "SubfileType".to_string(),
                    "Full-resolution image".to_string(),
                ),
            ]),
        );
        // A non-EXIF family in the middle must not disturb the resolution order,
        // and must not contribute anything.
        grouped.insert(
            "XMP-xmp".to_string(),
            BTreeMap::from([("ImageWidth".to_string(), "not metadata".to_string())]),
        );

        assert_eq!(
            exif_map_from(&grouped),
            BTreeMap::from([
                ("ImageWidth".to_string(), "3040".to_string()),
                (
                    "SubfileType".to_string(),
                    "Full-resolution image".to_string()
                ),
            ])
        );
    }

    /// A grouped read keeps every family and loses only what is not metadata.
    ///
    /// This is the shape the native-field mapping consumes, so what it drops
    /// decides what Iteration 2 has to ask `ExifTool` for a second time: the
    /// echoed path, entries with no value, and record-level fields that carry no
    /// group prefix. Everything with a group is kept, including the families
    /// `exifVec` has no use for — narrowing is the caller's job, and
    /// [`exif_map_from`] is the one that narrows.
    #[test]
    fn a_grouped_read_files_every_family_and_drops_the_echoed_path() {
        let grouped = group_by_family(&json!({
            "SourceFile": "/library/IMG_0001.jpg",
            "ExifTool:ExifToolVersion": 13.59,
            "System:FileName": "IMG_0001.jpg",
            "IFD0:Make": "FUJIFILM",
            "ExifIFD:DateTimeOriginal": "2024-05-06 07:08:09",
            "IFD0:GPSLatitudeRef": null,
            "XMP-dc:Subject": ["alpine", "winter"],
            "IPTC:Keywords": "alpine",
        }));

        assert_eq!(
            grouped.get("IFD0"),
            Some(&BTreeMap::from([(
                "Make".to_string(),
                "FUJIFILM".to_string()
            )])),
            "a tag ExifTool reported as valueless is absent, not stored as \"null\": {grouped:?}"
        );
        assert_eq!(
            grouped.get("XMP-dc"),
            Some(&BTreeMap::from([(
                "Subject".to_string(),
                "alpine, winter".to_string()
            )])),
            "the native-field mapping needs the XMP families spelled as they arrive: {grouped:?}"
        );
        assert_eq!(
            grouped.get("IPTC"),
            Some(&BTreeMap::from([(
                "Keywords".to_string(),
                "alpine".to_string()
            )])),
            "IPTC is the second precedence source and has to survive the split: {grouped:?}"
        );
        assert!(
            !grouped.values().any(|tags| tags.contains_key("SourceFile")),
            "the read path must not be stored as metadata: {grouped:?}"
        );

        // The families `exifVec` has no use for are still in the grouped record
        // and are dropped by the projection, not by the read.
        assert!(grouped.contains_key("System"), "{grouped:?}");
        assert_eq!(
            exif_map_from(&grouped),
            BTreeMap::from([
                (
                    "DateTimeOriginal".to_string(),
                    "2024-05-06 07:08:09".to_string()
                ),
                ("Make".to_string(), "FUJIFILM".to_string()),
            ]),
            "exifVec is the EXIF family alone"
        );
    }

    /// The `-G1` read and ExifTool's own ungrouped EXIF read must produce the same
    /// `exifVec`, field for field and character for character.
    ///
    /// The engine swap from a cold `exiftool -j -EXIF:all` spawn to a
    /// `-stay_open` session reading `-j -G1` and filtering client-side is only
    /// safe if the projection is exact, and "exact" is not something a handful
    /// of `assert_eq!`s can show. So this test keeps the old invocation as an
    /// oracle and compares whole maps, over every format the app indexes: the
    /// two pinned fixtures, a generated JPEG carrying XMP and IPTC as well as
    /// EXIF, and a file with no EXIF at all.
    ///
    /// It also pins the two behaviours that are easy to lose in a rewrite: the
    /// dash-shaped dates `-d` produces, and the fact that a file whose only
    /// metadata is non-EXIF still yields an empty map.
    #[test]
    fn the_grouped_read_matches_the_ungrouped_exif_read_byte_for_byte() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tagged = dir.path().join("tagged.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(tagged.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: Some(vec!["parity_alpine".into(), "parity_winter".into()]),
            exif_date: Some("2024:05:06 07:08:09".into()),
            minimal: false,
        }])
        .expect("generate photo");

        let not_an_image = dir.path().join("notes.jpg");
        std::fs::write(&not_an_image, b"this is not an image at all").expect("write file");

        for (label, path) in [
            ("pinned tiff", pinned_fixture("tiff-48x32-exif")),
            ("pinned webp", pinned_fixture("webp-48x32-exif")),
            ("tagged jpeg", tagged),
            ("not an image", not_an_image),
        ] {
            let ours = exif_vec_of(&path);
            let theirs = ungrouped_exif_read(&path);

            assert_eq!(
                ours, theirs,
                "exifVec for the {label} must be exactly what `exiftool -j -EXIF:all` reports"
            );
        }
    }

    /// ExifTool's own ungrouped EXIF read, used as the parity oracle: a cold
    /// `exiftool -j -EXIF:all -d <fmt>` spawn, the invocation this change
    /// replaced.
    fn ungrouped_exif_read(path: &Path) -> BTreeMap<String, String> {
        let output = std::process::Command::new(super::EXIFTOOL)
            .args(["-j", "-EXIF:all", "-d", super::EXIFTOOL_DATE_FORMAT])
            .arg(path)
            .output()
            .expect("the exiftool precondition test covers the binary being present");

        let mut map = BTreeMap::new();
        if !output.status.success() {
            return map;
        }
        let records: Vec<BTreeMap<String, serde_json::Value>> =
            serde_json::from_slice(&output.stdout).expect("exiftool -j emits a JSON array");
        if let Some(record) = records.first() {
            for (key, value) in record {
                if key == super::SOURCE_FILE_KEY {
                    continue;
                }
                if let Some(text) = json_value_to_string(value) {
                    map.insert(key.clone(), text);
                }
            }
        }
        map
    }

    /// Only a transport failure is worth a restart.
    ///
    /// The restart costs a fresh Perl startup, so it must not be spent on a file
    /// `ExifTool` cannot read — retrying that on a new child reaches the same
    /// answer more slowly. It must be spent on a dead child, which is the one
    /// failure a retry can actually fix.
    #[test]
    fn only_transport_failures_are_worth_a_restart() {
        // A dead child's pipes: `Io`, a disconnected stderr reader, and the
        // documented-but-never-constructed `ProcessTerminated`.
        assert!(is_transport_failure(&ExifToolError::Io(
            std::io::Error::from(std::io::ErrorKind::BrokenPipe)
        )));
        assert!(is_transport_failure(&ExifToolError::StderrDisconnected));
        assert!(is_transport_failure(&ExifToolError::ProcessTerminated));
        // The error the crate actually returns for a child that exits mid-read.
        assert!(is_transport_failure(&ExifToolError::ExifToolProcess {
            message: "Process terminated unexpectedly.".to_string(),
            std_err: String::new(),
            command_args: "-json -G1".to_string(),
        }));

        // Everything a *file* can do to the read.
        for err in [
            ExifToolError::FileNotFound {
                path: PathBuf::from("/library/absent.jpg"),
                command_args: "-json -G1 /library/absent.jpg".to_string(),
            },
            ExifToolError::ExifToolProcess {
                message: "Error: Malformed APP1 EXIF segment".to_string(),
                std_err: "Error: Malformed APP1 EXIF segment".to_string(),
                command_args: "-json -G1 broken.jpg".to_string(),
            },
            ExifToolError::Json(serde_json::from_str::<serde_json::Value>("{").unwrap_err()),
            ExifToolError::Utf8(String::from_utf8(vec![0xff]).expect_err("invalid UTF-8")),
            ExifToolError::UnexpectedFormat {
                path: String::new(),
                command_args: "-json -G1".to_string(),
            },
        ] {
            assert!(
                !is_transport_failure(&err),
                "{err} is about the file, not the child, so a restart cannot change it"
            );
        }
    }

    /// The grouped read is the plumbing the native-field mapping is built on, so
    /// it has to be shown reaching real XMP and IPTC data on a real file — not
    /// only a hand-written JSON object.
    ///
    /// `tags` makes snapfab write both an XMP packet and an APP13 IPTC block, so
    /// the assertions are about metadata that is really in the file. What is
    /// *not* asserted is which family a given field lands in: that mapping is
    /// the next iteration's decision to make, and pinning the family names here
    /// would only make that change a test edit.
    #[test]
    fn a_grouped_read_of_a_tagged_jpeg_carries_both_other_families() {
        let dir = tempfile::tempdir().expect("temp dir");
        let photo = dir.path().join("tagged.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(photo.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: Some(vec!["grouped_alpine".into(), "grouped_winter".into()]),
            exif_date: Some("2024:05:06 07:08:09".into()),
            minimal: false,
        }])
        .expect("generate photo");

        let grouped = group_by_family(
            &read_metadata_record(&photo).expect("a readable file is not an error"),
        );

        for family in ["XMP", "IPTC"] {
            assert!(
                grouped
                    .keys()
                    .any(|group| group == family || group.starts_with(&format!("{family}-"))),
                "the file carries {family} and one read has to surface it: {grouped:?}"
            );
        }
        assert!(
            grouped
                .iter()
                .any(|(group, tags)| group.starts_with("XMP") && tags.contains_key("Subject")),
            "the XMP subject list is the field the native mapping reads first: {grouped:?}"
        );
    }

    /// A missing binary has to stay a *named* failure, not a generic one.
    ///
    /// The indexer swallows a read error into an empty map, so this message is
    /// the only place the cause reaches an operator: it has to say
    /// the binary is missing and how to install it, on top of whatever the
    /// `exiftool` crate reports underneath.
    #[test]
    fn a_missing_binary_is_reported_with_the_install_remedy() {
        let Err(err) = Session::start_at(Path::new("/nonexistent/picasu-no-such-exiftool")) else {
            panic!("starting a session from a missing binary must fail");
        };

        // Typed, so a caller can tell "no binary" from "this file is unreadable".
        assert!(
            err.chain().any(|cause| {
                cause
                    .downcast_ref::<ExifToolError>()
                    .is_some_and(|typed| matches!(typed, ExifToolError::ExifToolNotFound(_)))
            }),
            "the missing binary must stay distinguishable as ExifToolNotFound: {err:#}"
        );

        let rendered = format!("{err:#}");
        for expected in [
            "exiftool",
            "just install-exiftool",
            "apt-get install libimage-exiftool-perl",
        ] {
            assert!(
                rendered.contains(expected),
                "the diagnostic should mention {expected:?}: {rendered}"
            );
        }
    }

    /// Killing the child underneath the session must not cost a file its EXIF.
    ///
    /// The session is a long-lived child on a long-lived server, and nothing
    /// else in the process watches it: it can be killed out of band (an
    /// operator, an OOM killer, a `pkill exiftool`), and every read after that
    /// would fail forever if the session were not replaced. `exiftool` does not
    /// exit on its own, so the only way to test this is to kill it from outside
    /// — which is what makes the test worth having.
    ///
    /// What is asserted, in order: the read works; SIGKILL on the child makes
    /// the *unretried* read fail, which is what proves the kill really broke the
    /// session and that the recovery below is the restart's doing and not luck;
    /// the retried read returns exactly what the live session returned; and it
    /// returned on a *different* child, which rules out a retry that quietly
    /// reused the dead session.
    ///
    /// The child is found through `/proc/thread-self/children` — this thread's
    /// own children, so a test thread running in parallel with the other
    /// `exiftool` tests owns exactly one of them. The `exiftool` crate does not
    /// expose a pid, which is why this goes through `/proc`.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_killed_child_is_replaced_and_the_next_read_still_succeeds() {
        let path = pinned_fixture("tiff-48x32-exif");
        let before = exiftool_children_of_this_thread();

        let mut session = Session::start().expect("start a session");
        let spawned: Vec<u32> = exiftool_children_of_this_thread()
            .into_iter()
            .filter(|pid| !before.contains(pid))
            .collect();
        assert_eq!(
            spawned.len(),
            1,
            "the session must own exactly one exiftool child, found {spawned:?} among {before:?}"
        );
        let child = spawned[0];

        // Control: the live session reads the fixture.
        let live = session
            .read(&path)
            .expect("a fresh session reads the fixture");
        assert_eq!(
            exif_map_from(&group_by_family(&live))
                .get("ImageWidth")
                .map(String::as_str),
            Some("48"),
            "control: the fixture reads as EXIF before the kill: {live}"
        );

        // SIGKILL: no chance for ExifTool to flush or close its pipes cleanly.
        let killed = std::process::Command::new("kill")
            .args(["-9", &child.to_string()])
            .status()
            .expect("run kill");
        assert!(killed.success(), "killing the child {child} must succeed");
        wait_until_gone(child);

        // The dead session is dead: without the restart this read is where the
        // server would start returning empty `exifVec` for every image.
        let broken = session
            .read(&path)
            .expect_err("a read through a killed child must fail");
        assert!(
            is_transport_failure(&broken),
            "a killed child has to be classified as a transport failure, got: {broken}"
        );

        // The retry has to be transparent: same input, same contract. The
        // comparison is on the projection, not the raw record, because a record
        // carries the file's access time and reading the file changes it.
        let recovered = session
            .read_retrying(&path)
            .expect("a restarted session recovers the read");
        assert_eq!(
            exif_map_from(&group_by_family(&recovered)),
            exif_map_from(&group_by_family(&live)),
            "a restarted session must read the file to the same result"
        );

        let replacement: Vec<u32> = exiftool_children_of_this_thread()
            .into_iter()
            .filter(|pid| *pid != child)
            .collect();
        assert_eq!(
            replacement.len(),
            1,
            "the retry must run on exactly one new child, found {replacement:?}"
        );
    }

    /// PIDs of this thread's direct children that are `exiftool -stay_open`
    /// sessions.
    ///
    /// Children are tracked per thread by the kernel, and a session is created
    /// on the thread that reads with it, so this sees one session's child and
    /// not the ones the other `exiftool` tests are using in parallel.
    #[cfg(target_os = "linux")]
    fn exiftool_children_of_this_thread() -> Vec<u32> {
        let Ok(listed) = std::fs::read_to_string("/proc/thread-self/children") else {
            panic!("reading /proc/thread-self/children");
        };
        listed
            .split_whitespace()
            .filter_map(|field| {
                let pid = field.parse::<u32>().ok()?;
                let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
                let cmdline = String::from_utf8_lossy(&cmdline);
                (cmdline.contains("exiftool") && cmdline.contains("-stay_open")).then_some(pid)
            })
            .collect()
    }

    /// Block until `pid` is dead, or fail the test.
    ///
    /// A SIGKILLed child stays in `/proc` as a zombie until its parent reaps it,
    /// and only the `exiftool` crate holds the `Child` handle, so "gone" has to
    /// mean "not running" rather than "no such directory".
    #[cfg(target_os = "linux")]
    fn wait_until_gone(pid: u32) {
        for _ in 0..500 {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            let state = stat
                .rsplit_once(") ")
                .and_then(|(_, rest)| rest.split_whitespace().next());
            if state.is_none() || state == Some("Z") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("exiftool child {pid} is still running after SIGKILL");
    }
}
