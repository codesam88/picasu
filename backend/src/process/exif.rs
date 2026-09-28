use crate::model::abstract_data::AbstractData;
use anyhow::{Context, Result, anyhow};
use exiftool::{ExifTool, ExifToolError};
use regex::Regex;
use serde_json::Value;
use std::{
    cell::RefCell, collections::BTreeMap, io, path::Path, path::PathBuf, process::Command,
    sync::LazyLock,
};

/// The metadata engine for image EXIF. `ExifTool` is an external binary (pure
/// Perl, invoked as a separate process like `ffprobe` below), and there is no
/// in-process fallback: [`read_metadata_record`] is the only reader of image
/// metadata. A file whose metadata cannot be read yields an empty `exifVec` and
/// still indexes; a **toolchain** that cannot be read at all does not, and
/// leaves the pipeline as an error — see [`is_toolchain_failure`].
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
/// a read that failed *about the file* into an empty map, which is what keeps a
/// broken image from turning into a rejected asset. The API-level counterpart is
/// `corrupt_exif_in_decodable_image_yields_empty_exif_vec`. A read that failed
/// about the **toolchain** is not absorbed — see [`is_toolchain_failure`].
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
/// child broke, or the output was not a JSON record. Whether the caller must
/// propagate that is [`is_toolchain_failure`]'s question; a file `ExifTool`
/// cannot parse is not an error here at all — it comes back as a record with no
/// EXIF groups, which the callers render as an empty map.
pub(crate) fn read_metadata_record(file_path: &Path) -> Result<Value> {
    read_with_the_engine(file_path)
        .with_context(|| format!("failed to read metadata for {}", file_path.display()))
}

/// One read through this thread's session, with no error policy applied.
///
/// Split from [`read_metadata_record`] so the file context and the engine's own
/// error stay separable — the remedy attaches to one and not the other — and so
/// the test seam has one place to stand in for the engine.
fn read_with_the_engine(file_path: &Path) -> Result<Value> {
    #[cfg(test)]
    if let Some(injected) = INJECTED_READ.with(|slot| slot.borrow_mut().take()) {
        return injected.map_err(engine_failure);
    }

    SESSION.with(|slot| -> Result<Value> {
        let mut slot = slot.borrow_mut();
        // A session that could not be started is never cached, so a missing
        // binary is reported per read instead of latching as a dead session.
        if slot.is_none() {
            *slot = Some(Session::start()?);
        }
        let session = slot.as_mut().expect("session installed just above");
        session.read_retrying(file_path)
    })
}

/// One property assignment for [`write_xmp_properties`]: an `ExifTool` tag name
/// (`XMP-dc:Subject`) and the value to give it, or `None` to remove it.
///
/// An empty `Some("")` is *not* how a removal is spelled. `ExifTool` takes
/// `-TAG=` as "delete this property" — measured, and the only spelling that
/// removes one — so the two are kept apart here: `None` means delete, and an
/// empty string is a value like any other.
pub(crate) struct XmpAssignment<'a> {
    pub(crate) tag: &'a str,
    pub(crate) value: Option<&'a str>,
}

/// What a write through [`write_xmp_properties`] actually did.
///
/// Not every outcome is a success, and the difference decides what the caller
/// is allowed to do about it, so the write's own result rather than a bare `()`
/// is what comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum XmpWriteOutcome {
    /// The properties were written into the file's existing packet.
    Updated,
    /// No packet was found, so `ExifTool` wrote one — either because the file
    /// was absent or empty, or because what it held was not an XMP packet
    /// `ExifTool` recognises. The caller's file was therefore replaced whole.
    Created,
    /// `ExifTool` read the file and refused to write to it. The file is
    /// byte-identical to what it was: `ExifTool` stages the new packet in a
    /// temporary file and renames it over the target only on success, so a
    /// rejected write costs the caller nothing and tells it the file's own
    /// metadata is what `ExifTool` cannot handle.
    Rejected,
}

/// Write XMP properties into the packet at `file_path`, leaving every property
/// this call does not name exactly as it was — the read-modify-write primitive
/// `process::xmp_write` builds the sidecar contract on.
///
/// The write goes through the same thread-local session as [`read_metadata_record`],
/// and for the same reason: a sidecar write happens on every tag, description,
/// rating and album edit, and a cold `exiftool` costs ~330 ms against ~3 ms
/// through a live child. Sharing the session also means a thread that reads and
/// writes metadata keeps one child, not two.
///
/// `ExifTool` is the writer rather than a hand-rolled serialiser (decision 7 of
/// `.plan/exiftool-metadata-engine.md`): a packet it edits is re-serialised by
/// the tool that owns the format, which is what makes the properties it does
/// not understand — unknown namespaces, another tool's ratings, edit history —
/// survive an edit at all.
///
/// ## Argument shape, measured against `ExifTool` 13.59
///
/// * `assignments` become one `-<tag>=<value>` argument each, and **one
///   assignment per list element**. A list-valued tag is *replaced* by its
///   assignments, not appended to: `-XMP-dc:Subject=alpha` against a bag
///   holding `[oldtag, keepme]` leaves `[alpha]`. The append spellings are
///   `-<tag>+=<value>` and the remove-one is `-<tag>-=<value>`, neither of
///   which is used here, so a rewritten tag set cannot accumulate stale tags
///   across edits.
/// * `value: None` becomes `-<tag>=`, which deletes the property. On a file
///   that does not have it, that is a no-op `ExifTool` reports as
///   `1 image files unchanged` — so clearing an absent managed field does not
///   manufacture an empty element, and an album whose title was never
///   customised still gets no `dc:title`.
/// * `-overwrite_original` is mandatory: without it `ExifTool` leaves a
///   `<file>_original` backup beside every sidecar it touches.
/// * A value containing a newline cannot travel in an argument, because the
///   `exiftool` crate writes one argument per line of the `-stay_open` argument
///   file and a raw newline would split it into two — measured to write the
///   value truncated at the newline *and* report a spurious error for the
///   remainder. Backslash escaping is not a way out either: the argument file
///   is not shell-interpreted, so `\n` arrives as a literal backslash and `n`.
///   Such a value is staged in a file and assigned with `-<tag>=<path>`
///   instead, which round-trips the content exactly, newlines, quotes,
///   ampersands, backslashes and UTF-8 included.
///
/// ## Errors
///
/// `Err` means the write did not happen and says nothing about the file: no
/// session could be started, the transport to the persistent child broke, or
/// the child produced output this could not read. `ExifTool` rejecting the
/// *file* is not an error here — that is [`XmpWriteOutcome::Rejected`], which
/// carries a different remedy and is the caller's to act on.
pub(crate) fn write_xmp_properties(
    file_path: &Path,
    assignments: &[XmpAssignment<'_>],
) -> Result<XmpWriteOutcome> {
    SESSION.with(|slot| -> Result<XmpWriteOutcome> {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Session::start()?);
        }
        let session = slot.as_mut().expect("session installed just above");
        session
            .write_retrying(file_path, assignments)
            .with_context(|| format!("failed to write XMP to {}", file_path.display()))
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
    /// The read path and the write path share this one session, so a thread
    /// that does both keeps a single child rather than one per direction.
    ///
    /// The session is dropped, and its child killed, when the owning thread
    /// exits; there is no cross-thread sharing and no global state to reset
    /// between tests.
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };

    /// The executable [`Session::start`] spawns from, overridden per thread by
    /// [`with_read_seam`]. `None` is the production state and resolves to
    /// [`EXIFTOOL`].
    ///
    /// Thread-local for the same reason [`SESSION`] is: taking `exiftool` off
    /// the process-wide `PATH` would break every other test in the binary, since
    /// `PATH` is global and the suite runs in parallel.
    #[cfg(test)]
    static EXECUTABLE_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };

    /// A read outcome returned in place of the engine's own, set per thread by
    /// [`with_read_seam`] and consumed by the first read on that thread.
    ///
    /// This is the half of the seam a real file cannot provide. ExifTool 13.59
    /// reports a file it cannot parse *inside* the JSON record
    /// (`ExifTool:Error` / `ExifTool:Warning`) and still exits with a parseable
    /// record, so a damaged file is answered with `Ok` and no EXIF groups
    /// rather than with an error. A toolchain failure is the class a live file
    /// does produce, and a chosen variant is the only way to pin what the
    /// pipeline does with the *other* side of the classification.
    #[cfg(test)]
    static INJECTED_READ: RefCell<Option<Result<Value, ExifToolError>>> =
        const { RefCell::new(None) };
}

/// What a test makes the read layer do instead of talking to the engine.
///
/// A `#[cfg(test)]` seam on the read boundary, for the two failures a real
/// library cannot produce on demand: a deployment whose `exiftool` cannot start,
/// and a chosen error variant. Taking `exiftool` off the process-wide `PATH`
/// would be the obvious alternative and is unusable — `PATH` is global, the
/// suite runs in parallel, and every other `exiftool` test would start failing.
#[cfg(test)]
pub(crate) enum ReadSeam {
    /// Start the session from this executable instead of [`EXIFTOOL`], so a
    /// spawn failure is a real `ExifToolNotFound` from the real code path.
    Executable(PathBuf),
    /// Answer the next read with this outcome instead of running the engine.
    Injected(Result<Value, ExifToolError>),
}

/// Run `body` with the read layer redirected by `seam`, then restore it.
///
/// The override is thread-local and covers one `#[test]`, which runs on its own
/// thread; the session is dropped on the way out so a session started under the
/// seam cannot outlive it. A panic in `body` still leaves the thread's override
/// installed, which costs nothing: the thread is the test's own and ends with it.
#[cfg(test)]
pub(crate) fn with_read_seam<T>(seam: ReadSeam, body: impl FnOnce() -> T) -> T {
    match seam {
        ReadSeam::Executable(executable) => {
            EXECUTABLE_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(executable));
        }
        ReadSeam::Injected(outcome) => {
            INJECTED_READ.with(|slot| *slot.borrow_mut() = Some(outcome));
        }
    }
    let out = body();
    EXECUTABLE_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    INJECTED_READ.with(|slot| *slot.borrow_mut() = None);
    SESSION.with(|slot| *slot.borrow_mut() = None);
    out
}

/// The executable a session on this thread is spawned from.
///
/// [`EXIFTOOL`] in production, whatever [`with_read_seam`] installed for a test
/// run on the same thread.
fn executable_for_this_thread() -> PathBuf {
    #[cfg(test)]
    if let Some(override_path) = EXECUTABLE_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return override_path;
    }
    PathBuf::from(EXIFTOOL)
}

/// A running `exiftool -stay_open` child plus the one read that has to survive
/// its death.
struct Session {
    tool: ExifTool,
}

impl Session {
    /// Spawn the persistent child, resolving it on `PATH` for this thread.
    fn start() -> Result<Self> {
        Self::start_at(&executable_for_this_thread())
    }

    /// Spawn the persistent child from `executable`, attaching the install
    /// remedy to a failure.
    ///
    /// A binary that is missing or not runnable is reported as
    /// `ExifToolNotFound` carrying the OS error underneath, which is what makes
    /// the cause nameable in [`toolchain_diagnostic`]; taking the path as an
    /// argument rather than hardcoding it is what lets a test observe that error
    /// without taking `exiftool` off the process-wide `PATH` for every other
    /// test.
    ///
    /// A failure is also logged, because the read layer is not the only thing
    /// that starts a session and the log is where an operator meets the cause
    /// outside an indexing response. Since the pipeline now propagates a
    /// toolchain failure, the log is no longer the only signal — it fires per
    /// *session creation* rather than per file, and a session that fails to
    /// start is never cached (see [`read_metadata_record`]), so with the binary
    /// missing it repeats once per failing read.
    fn start_at(executable: &Path) -> Result<Self> {
        ExifTool::with_executable(executable)
            .map(|tool| Session { tool })
            .map_err(engine_failure)
            .inspect_err(|err| {
                log::error!("image metadata is unavailable: {err:#}");
            })
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
    ///
    /// The retry's failure is the one that reaches a caller, and it goes through
    /// [`engine_failure`] like every other engine error, so a child that dies
    /// again — or a binary that has since been removed — arrives with the
    /// install remedy attached rather than as a bare pipe error.
    fn read_retrying(&mut self, file_path: &Path) -> Result<Value> {
        match self.read(file_path) {
            Err(err) if is_transport_failure(&err) => {
                *self = Session::start()?;
                self.read(file_path).map_err(engine_failure)
            }
            other => Ok(other?),
        }
    }

    fn write(
        &self,
        file_path: &Path,
        assignments: &[XmpAssignment<'_>],
    ) -> Result<XmpWriteOutcome, ExifToolError> {
        // Values that cannot survive one line of the argument file are staged
        // beside the target and read back through `<=`, so the assignment list
        // below can stay owned by the caller. `staged` is dropped on every path
        // out of here, which is what removes the value files.
        let staged = StagedValues::new(file_path, assignments)?;
        let path = file_path.to_string_lossy().into_owned();
        let mut args = vec![WRITE_FLAG];
        args.extend(staged.arguments().iter().map(String::as_str));
        args.push(&path);
        // `execute_raw` reports a non-empty stderr as an error, so the outcome
        // is decided from the child's stdout and the `Err` is only consulted for
        // whether anything at all came back.
        match self.tool.execute_raw(&args) {
            // ExifTool prints its verdict on stdout, in the same `-execute`
            // block as the counts, so this is the channel to read it from.
            Ok(stdout) => Ok(classify_write(&String::from_utf8_lossy(&stdout))),
            // The child is gone, which says nothing about the file: the caller
            // restarts and retries.
            Err(err) if is_transport_failure(&err) => Err(err),
            // The crate turns *any* stderr into an `Err` and drops the stdout it
            // already read, so a verdict can arrive on this channel instead. It
            // is decidable, because ExifTool's two severities mean opposite
            // things: an `Error:` line is a refused write, the file untouched,
            // while a `Warning:` on its own accompanies a write that *landed* —
            // measured, writing `XMP-dc:Subject` over a packet that held a
            // scalar there fixes the list type and updates the file. Reading a
            // warning as a refusal would replace a readable packet with a bare
            // one, which is the loss of unmanaged data this whole path exists
            // to prevent.
            Err(ExifToolError::ExifToolProcess { message, .. }) => {
                if reports_a_failure(&message) {
                    Ok(XmpWriteOutcome::Rejected)
                } else {
                    log::warn!("{message}; the write landed, so the packet is kept as written");
                    Ok(XmpWriteOutcome::Updated)
                }
            }
            Err(err) => Err(err),
        }
    }

    /// Write through the session, restarting the child at most once.
    ///
    /// The same transport-versus-file split as [`Session::read_retrying`], and
    /// for the same reason: a dead child is the one failure a fresh child can
    /// fix, while a file `ExifTool` refuses reaches the same answer more slowly
    /// on a new one.
    fn write_retrying(
        &mut self,
        file_path: &Path,
        assignments: &[XmpAssignment<'_>],
    ) -> Result<XmpWriteOutcome> {
        match self.write(file_path, assignments) {
            Err(err) if is_transport_failure(&err) => {
                *self = Session::start()?;
                Ok(self.write(file_path, assignments)?)
            }
            other => Ok(other?),
        }
    }
}

/// The one argument every write carries, and the reason a write cannot be
/// quiet about it: without it `ExifTool` leaves a `<file>_original` backup
/// beside each sidecar it touches.
const WRITE_FLAG: &str = "-overwrite_original";

/// The line `ExifTool` prints when it declined to write to a file, and the only
/// signal that distinguishes a refusal from a no-op.
///
/// It has to be this line and not the `0 image files updated` beside it:
/// clearing a property the file does not have also reports zero files updated,
/// alongside `1 image files unchanged`, and that is a success.
const WRITE_REFUSED: &str = "files weren't updated due to errors";

/// The line `ExifTool` prints when it found no packet to write into.
const WRITE_CREATED: &str = "image files created";

/// The prefix `ExifTool` puts on a stderr line that reports a failure, as
/// opposed to the `Warning:` prefix that reports a repair it made on the way to
/// a successful write.
const STDERR_ERROR_PREFIX: &str = "Error:";

/// Read a write's outcome off the child's stdout.
///
/// The `exiftool` crate's own error handling cannot answer this on its own. It
/// polls the child's stderr for two milliseconds and turns anything it caught
/// into an error, which is both too eager — a `Warning:` about a corrected list
/// type accompanies a write that succeeded — and not sufficient on its own,
/// since a slow stderr reader can miss the `Error:` line and hand back an `Ok`
/// whose stdout the caller has to read. `ExifTool` prints the verdict on stdout in
/// the same `-execute` block as the counts, so stdout is what this reads and
/// [`reports_a_failure`] is the fallback for when the crate's error swallowed
/// it.
fn classify_write(stdout: &str) -> XmpWriteOutcome {
    if stdout.contains(WRITE_REFUSED) {
        XmpWriteOutcome::Rejected
    } else if stdout.contains(WRITE_CREATED) {
        XmpWriteOutcome::Created
    } else {
        XmpWriteOutcome::Updated
    }
}

/// Whether what `ExifTool` wrote to stderr contains a line reporting a failure
/// rather than a warning.
fn reports_a_failure(stderr: &str) -> bool {
    stderr
        .lines()
        .any(|line| line.trim_start().starts_with(STDERR_ERROR_PREFIX))
}

/// The assignment arguments for one write, with the values that cannot travel
/// in an argument staged in files.
///
/// `ExifTool`'s argument file is line-oriented and is not shell-interpreted, so
/// an embedded newline is unrepresentable in an argument: measured, it writes
/// the value truncated at the newline and separately reports an error for the
/// remainder, which would read as a rejected write for a file that is fine. A
/// value containing a newline is therefore written to `<target>.picasu-value`
/// and assigned with `-<tag>=<path>`, which `ExifTool` reads as the property's
/// new value in full.
struct StagedValues<'a> {
    /// The `-<tag>=<value>` / `-<tag>=<path>` arguments, in assignment order.
    arguments: Vec<String>,
    /// The staged value files, removed when this value is dropped so that a
    /// write which fails, succeeds or panics leaves none of them behind.
    staged: Vec<PathBuf>,
    /// Kept alive so `StagedValues` can borrow the assignments it renders.
    _assignments: std::marker::PhantomData<&'a [XmpAssignment<'a>]>,
}

impl<'a> StagedValues<'a> {
    fn new(
        target: &Path,
        assignments: &'a [XmpAssignment<'a>],
    ) -> Result<StagedValues<'a>, ExifToolError> {
        let mut staged = StagedValues {
            arguments: Vec::with_capacity(assignments.len()),
            staged: Vec::new(),
            _assignments: std::marker::PhantomData,
        };
        for assignment in assignments {
            let argument = staged.argument_for(target, assignment)?;
            staged.arguments.push(argument);
        }
        Ok(staged)
    }

    /// The assignment's one argument, staging the value in a file when it
    /// carries a line break.
    fn argument_for(
        &mut self,
        target: &Path,
        assignment: &XmpAssignment<'_>,
    ) -> Result<String, ExifToolError> {
        // A removal is `-<tag>=`, and it is the only spelling that removes one.
        let Some(value) = assignment.value else {
            return Ok(format!("-{}=", assignment.tag));
        };
        if !value.contains(['\n', '\r']) {
            return Ok(format!("-{}={}", assignment.tag, value));
        }
        let path = staged_value_path(target, self.staged.len());
        std::fs::write(&path, value.as_bytes()).map_err(ExifToolError::Io)?;
        self.staged.push(path.clone());
        Ok(format!("-{}<={}", assignment.tag, path.to_string_lossy()))
    }

    fn arguments(&self) -> &[String] {
        &self.arguments
    }
}

impl Drop for StagedValues<'_> {
    fn drop(&mut self) {
        for path in &self.staged {
            // A leftover value file would be picked up as a sidecar on the next
            // index, so failing to remove one is worth saying out loud rather
            // than swallowing.
            if let Err(err) = std::fs::remove_file(path)
                && err.kind() != io::ErrorKind::NotFound
            {
                log::warn!(
                    "failed to remove staged XMP value file {}: {err}",
                    path.display()
                );
            }
        }
    }
}

/// Where the `n`th staged value of a write to `target` lives.
///
/// A sibling of the target rather than a temporary directory file, because
/// `ExifTool` resolves a relative path against its own working directory and an
/// absolute one is what keeps this independent of where the child was started.
/// The name is derived from the target so two concurrent writes to two
/// different sidecars in one directory cannot collide, and it is not a `.xmp`
/// extension so an interrupted write can never be mistaken for a sidecar.
fn staged_value_path(target: &Path, index: usize) -> PathBuf {
    let name = target.file_name().map_or_else(
        || "sidecar".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let stem = target.with_extension("");
    let dir = stem.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!(".{name}.picasu-value{index}"))
}

/// Whether `err` says the persistent child is gone rather than the file being
/// unreadable, and so whether retrying on a fresh child can help.
fn is_transport_failure(err: &ExifToolError) -> bool {
    match err {
        // Every way the crate can say the session is gone rather than the file
        // being unreadable: writing to or reading from a dead child's pipes, a
        // stderr reader that ended with the child, a mutex the crate's own lock
        // left poisoned (a fresh session has a fresh one), and the documented
        // `ProcessTerminated` — which the crate never actually constructs, so
        // the end-of-stream error it reports instead is matched on its message
        // too.
        ExifToolError::Io(_)
        | ExifToolError::StderrDisconnected
        | ExifToolError::ProcessTerminated
        | ExifToolError::MutexPoison(_) => true,
        ExifToolError::ExifToolProcess { message, .. } => reports_a_dead_child(message),
        _ => false,
    }
}

/// The message the `exiftool` crate reports for a child that exited while a
/// read was in flight, which is how a dead child actually reaches a caller
/// despite the variant it documents for it never being constructed.
fn reports_a_dead_child(message: &str) -> bool {
    message.contains("terminated unexpectedly")
}

/// Whether an engine failure is about the **toolchain** rather than about the
/// file that was being read — the classification every caller branches on.
///
/// This is the whole of the missing-`exiftool` contract, and the distinction is
/// the difference between a deployment that is broken and a file that is. Before
/// it existed the two were the same stored state, `exifVec: {}` on an image
/// nobody had read any metadata from, and the only difference was a log line.
///
/// ## The table
///
/// **Toolchain** — nothing about this file was learned, and the same failure
/// will happen for the next one:
///
/// * `ExifToolNotFound` — the binary is missing or not runnable. This is the
///   case the plan named, and the one that used to be invisible.
/// * every [`is_transport_failure`] — the session could not carry the exchange.
///   This is where `Io` lands, and the measurement settles it: the read path is
///   `json` → `json_batch` → `json_execute` → `execute_raw`, and `Io` is
///   constructed in exactly three places in it — the argument writes to the
///   child's stdin, the flush after them, and the read from its stdout. `ExifTool`
///   opens the file itself, in its own process, and reports anything wrong with
///   it on stderr or inside the record, so an `Io` from this path cannot be
///   about the file's contents or its permissions. It is the pipes.
///
/// **File** — the engine ran and this file is the reason, so the file is the
/// only thing that loses:
///
/// * `ExifToolProcess` — `ExifTool` put an `Error:` line on stderr, i.e. it
///   read the exchange and rejected the file. The dead-child message of the same
///   variant is the exception above, matched on [`reports_a_dead_child`].
/// * `FileNotFound` — the path was not there for the child to open, which is a
///   per-file condition: a file deleted between the scan and the read, or a
///   record whose path was never set.
/// * `Json`, `Utf8`, `UnexpectedFormat` — output the engine produced and this
///   code could not read. They are problems *about the answer*, not about the
///   toolchain's health, and `ExifTool` 13.59 in particular answers a file it
///   cannot parse by putting `ExifTool:Error` inside a parseable record rather
///   than on stderr — measured, so most of this arm is reached only by output
///   outside that shape.
/// * `Deserialization`, `TagNotFound`, `TagDeserialization` — the typed-read
///   variants. `read_metadata_record` uses none of those crate methods, so these
///   are unreachable from the read path today; they are grouped with the file
///   failures because that is the safer direction for a variant this code has
///   never seen — a per-file empty map rather than a library-wide indexing
///   failure — and the catch-all in [`is_transport_failure`] is the same bet.
fn is_toolchain_variant(err: &ExifToolError) -> bool {
    match err {
        ExifToolError::ExifToolNotFound(_) => true,
        other => is_transport_failure(other),
    }
}

/// Whether a read error is about the toolchain, as [`is_toolchain_variant`]
/// answers it for one [`ExifToolError`].
///
/// The chain is walked because the crate's error is the innermost of several:
/// [`read_metadata_record`] puts the file on the outside and [`engine_failure`]
/// puts the remedy between, so a caller that only inspected the outermost cause
/// would classify every failure as a fact about the file. Downcasting is what
/// makes the lookup work — the crate's error is a typed `#[source]`, and anyhow
/// preserves it through both context layers.
pub(crate) fn is_toolchain_failure(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<ExifToolError>()
            .is_some_and(is_toolchain_variant)
    })
}

/// The error one engine failure becomes, with the install remedy attached when
/// the failure is about the toolchain.
///
/// One function, so no path can produce a toolchain failure without a remedy:
/// the session start, the retried read and the test seam all come through it,
/// and there is one wording to keep current rather than one per call site.
fn engine_failure(err: ExifToolError) -> anyhow::Error {
    let is_toolchain = is_toolchain_variant(&err);
    let err = anyhow::Error::new(err);
    if is_toolchain {
        err.context(toolchain_diagnostic())
    } else {
        err
    }
}

/// The install remedy, and the text every toolchain-classified failure is
/// logged or returned with.
///
/// It is the actionable half of the contract: a propagated failure is only
/// better than a swallowed one if the reader of the message can tell whether to
/// install a binary, and the same failure is reachable from a log line, an
/// upload response, an album-scan counter and a rebuild's per-file diagnostics.
const INSTALL_REMEDY: &str =
    "install it with `just install-exiftool` (or `apt-get install libimage-exiftool-perl`)";

/// The operator-facing sentence for a toolchain failure: what is wrong, and what
/// to do about it.
///
/// `pub(crate)` because it is two answers to one question, not a log string. It
/// is the context [`engine_failure`] attaches to a propagated failure, and it is
/// what an HTTP response says when a request fails for this reason — the upload
/// handler returns it verbatim. One wording for both is the point: an operator
/// who reads the upload response and an operator who reads the log are told the
/// same thing, and a reworded remedy exists in exactly one place.
///
/// It carries no path, no hash and no crate-internal detail, because it is safe
/// to hand to a client. The specific file and the crate's own error stay in the
/// server log, where [`read_metadata_record`] puts them.
pub(crate) fn toolchain_diagnostic() -> String {
    format!(
        "image metadata is unavailable: {EXIFTOOL} could not be started, or its -stay_open \
         session could not serve a read after a restart; {INSTALL_REMEDY} and index again"
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

/// Flatten one `ExifTool` JSON value into the single string the app prints it as.
///
/// `ExifTool`'s JSON is not uniformly typed: `ExposureTime` arrives as the string
/// `"1/3188"`, `FNumber` as the number `19.7`, a multi-valued tag as an array.
/// `null` means "no value" and is dropped rather than stored as the text
/// `"null"`; an object cannot occur while `-struct` is not requested, and is
/// kept as JSON so nothing is silently lost if that ever changes.
///
/// Shared with the further-metadata bucket (`process::xmp::map_further_fields`)
/// rather than reimplemented there: the same record is rendered in two places in
/// one sidebar, and a value that `exifVec` printed one way and the bucket another
/// would be the same metadata shown two ways.
pub(crate) fn json_value_to_string(value: &Value) -> Option<String> {
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
/// Deliberately a hard failure rather than a silent skip. The pipeline now
/// propagates a toolchain failure, so a missing ExifTool does not store empty
/// `exifVec`s — but it does fail most of the suite, and a precondition that
/// explains itself is a better first failure than a screen of metadata
/// assertions failing on empty maps. `resolved` is a slice of `(tool,
/// path-on-PATH)` pairs so the message can be tested without removing a binary
/// from the environment.
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

/// Absolute path of a fixture pinned in the capability manifest, so a test
/// reads the same bytes the manifest's SHA-256 check covers and the scenarios
/// copy. Module level because the read layer, the pipeline and a rebuild all
/// need the same fixtures.
#[cfg(test)]
pub(crate) fn pinned_fixture_path(id: &str) -> PathBuf {
    let entry = snapfab::capabilities::capabilities()
        .fixture_by_id(id)
        .unwrap_or_else(|| panic!("fixture `{id}` should be registered in the manifest"));
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the backend manifest dir has a parent: the repository root")
        .join(&entry.path)
}

/// The JPEG EXIF header, the six bytes `Exif\0\0` an APP1 segment's EXIF
/// payload starts with.
#[cfg(test)]
const EXIF_HEADER: &[u8] = b"Exif\0\0";

/// The two byte-order words a TIFF block may start with, and the invalid word
/// that replaces whichever one a file carries.
#[cfg(test)]
const BYTE_ORDERS: [(&[u8], &[u8]); 2] = [(b"II*\0", b"XY*\0"), (b"MM\0*", b"XY\0*")];

/// Replace the TIFF byte-order word of `jpeg`'s EXIF block with an invalid one,
/// leaving the file length unchanged.
///
/// A file this has been applied to is the "damaged metadata inside an otherwise
/// decodable image" case: `ExifTool` reports `Malformed APP1 EXIF segment` and
/// extracts nothing, while the pixel data the rest of the pipeline decodes is
/// untouched. It lives at module level rather than in one test module because
/// the read layer, the pipeline and a rebuild all need the same fixture, and
/// three copies of a byte-level corruption is three chances to write the wrong
/// one.
///
/// Which of `II*\0` or `MM\0*` a block starts with is the fixture *writer's*
/// business, not the caller's: `little_exif` wrote little-endian, ExifTool
/// writes big-endian, and a file from a camera will be either, so the marker is
/// discovered and whichever it is gets damaged. Only the two marker letters are
/// replaced, and the corrupted word is one `ExifTool` has no fallback for:
/// patching the trailing `0` instead — the IFD offset, which for a big-endian
/// block shares the word — leaves every tag readable, because the directory is
/// then found by scanning.
#[cfg(test)]
pub(crate) fn corrupt_exif_byte_order(jpeg: &mut [u8]) {
    let at = jpeg
        .windows(EXIF_HEADER.len())
        .position(|window| window == EXIF_HEADER)
        .expect("the fixture carries an Exif\\0\\0 header");
    let word_at = at + EXIF_HEADER.len();
    let word = jpeg
        .get(word_at..word_at + 4)
        .expect("the EXIF header is followed by a byte-order word");
    let (_, corrupt) = BYTE_ORDERS
        .iter()
        .find(|(valid, _)| *valid == word)
        .unwrap_or_else(|| panic!("the EXIF block starts with an unknown byte order {word:?}"));
    jpeg[word_at..word_at + 4].copy_from_slice(corrupt);
}

#[cfg(test)]
mod tests {
    use super::{
        GroupedMetadata, PathBuf, ReadSeam, Session, WRITE_REFUSED, XmpAssignment, XmpWriteOutcome,
        check_exiftool_toolchain, classify_write, corrupt_exif_byte_order, engine_failure,
        exif_group_rank, exif_map_from, exif_map_from_record, group_by_family,
        is_toolchain_failure, is_toolchain_variant, is_transport_failure, json_value_to_string,
        pinned_fixture_path as pinned_fixture, read_metadata_record, resolve_exiftool_tools,
        staged_value_path, tool_runs, with_read_seam, write_xmp_properties,
    };
    use exiftool::ExifToolError;
    use serde_json::json;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
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

    /// The XMP properties `ExifTool` reports for `path`, read back through the
    /// production read path as `XMP-<ns>:<Tag>` → value. A tag the file does
    /// not carry is absent rather than empty, which is what lets a test say
    /// "this property is gone" and not "this property is blank".
    fn xmp_properties_of(path: &Path) -> BTreeMap<String, String> {
        let record = read_metadata_record(path).expect("the write must leave a readable file");
        let serde_json::Value::Object(entries) = &record else {
            panic!("a record is an object, as read_metadata_record returns one")
        };
        entries
            .iter()
            .filter(|(key, _)| key.starts_with("XMP-"))
            .filter_map(|(key, value)| Some((key.clone(), super::json_value_to_string(value)?)))
            .collect()
    }

    /// A sidecar carrying one managed property per namespace the app manages
    /// and, beside them, properties from namespaces it does not model at all.
    ///
    /// The unmanaged set is the point of the fixture: `photoshop:*` and
    /// `tiff:Make` are what another tool wrote, `Iptc4xmpCore:*` is a namespace
    /// the app has no read rule for, and `xmpRights:Marked` is a scalar of the
    /// *same* namespace the app writes a managed property in. A writer that
    /// rewrote a whole namespace, rather than the named properties in it, would
    /// pass on the first two and fail on the last.
    fn seeded_sidecar() -> String {
        [
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>"#,
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">"#,
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#,
            r#"<rdf:Description rdf:about="""#,
            r#"    xmlns:dc="http://purl.org/dc/elements/1.1/""#,
            r#"    xmlns:xmp="http://ns.adobe.com/xap/1.0/""#,
            r#"    xmlns:xmpRights="http://ns.adobe.com/xap/1.0/rights/""#,
            r#"    xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/""#,
            r#"    xmlns:tiff="http://ns.adobe.com/tiff/1.0/""#,
            r#"    xmlns:Iptc4xmpCore="http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/">"#,
            r#"  <dc:subject><rdf:Bag><rdf:li>stale-tag</rdf:li><rdf:li>also-stale</rdf:li></rdf:Bag></dc:subject>"#,
            r#"  <dc:description><rdf:Alt><rdf:li xml:lang="x-default">stale description</rdf:li></rdf:Alt></dc:description>"#,
            r#"  <xmp:Rating>1</xmp:Rating>"#,
            r#"  <xmpRights:Marked>True</xmpRights:Marked>"#,
            r#"  <photoshop:City>Zürich</photoshop:City>"#,
            r#"  <photoshop:Country>Switzerland</photoshop:Country>"#,
            r#"  <tiff:Make>SeedCam</tiff:Make>"#,
            r#"  <Iptc4xmpCore:Location>Alps</Iptc4xmpCore:Location>"#,
            r#"</rdf:Description>"#,
            r#"</rdf:RDF>"#,
            r#"</x:xmpmeta>"#,
            r#"<?xpacket end="w"?>"#,
            "",
        ]
        .join("\n")
    }

    /// Write `seeded_sidecar` to `dir/photo.xmp` and return the path.
    fn seed_sidecar_in(dir: &Path) -> PathBuf {
        let path = dir.join("photo.xmp");
        std::fs::write(&path, seeded_sidecar()).expect("seed the sidecar");
        path
    }

    /// The read-modify-write contract in one test: naming some properties
    /// changes exactly those and leaves every other property in the packet —
    /// same namespace or not — as it was.
    ///
    /// `ExifTool` re-serialises a packet it edits, so "preserved" is a
    /// property-level claim and not a byte-level one: the assertion is that the
    /// values come back through the reader unchanged, which is what a consumer
    /// of `furtherMetadata` or another tool observes.
    #[test]
    fn a_write_leaves_every_property_it_did_not_name_intact() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sidecar = seed_sidecar_in(dir.path());
        let before = xmp_properties_of(&sidecar);

        write_xmp_properties(
            &sidecar,
            &[
                XmpAssignment {
                    tag: "XMP-dc:Subject",
                    value: Some("fresh-tag"),
                },
                XmpAssignment {
                    tag: "XMP-dc:Description",
                    value: Some("a fresh description"),
                },
                XmpAssignment {
                    tag: "XMP:Rating",
                    value: Some("4"),
                },
            ],
        )
        .expect("the write should succeed");

        let after = xmp_properties_of(&sidecar);
        assert_eq!(
            after.get("XMP-dc:Subject"),
            Some(&"fresh-tag".to_string()),
            "the bag must equal the managed tag set"
        );
        assert_eq!(
            after.get("XMP-dc:Description"),
            Some(&"a fresh description".to_string())
        );
        // ExifTool writes `XMP:Rating` and reports it back under the *namespace*
        // it resolved, so the read key is not the write tag. Asserting the write
        // spelling here would pass on a write that never happened.
        assert_eq!(after.get("XMP-xmp:Rating"), Some(&"4".to_string()));
        assert!(
            !after.contains_key("XMP:Rating"),
            "the rating must not land in an unnamespaced group"
        );

        for key in [
            // Another tool's properties, in namespaces the app does not model.
            "XMP-photoshop:City",
            "XMP-photoshop:Country",
            "XMP-tiff:Make",
            "XMP-iptcCore:Location",
            // A property in a namespace the write *does* touch, which a writer
            // that replaced whole namespaces would have taken with it.
            "XMP-xmpRights:Marked",
        ] {
            assert_eq!(
                after.get(key),
                before.get(key),
                "{key} is unmanaged and must survive the write unchanged"
            );
            assert!(
                before.contains_key(key),
                "the seed must carry {key}, or the assertion above is vacuous"
            );
        }
    }

    /// The core correctness property of replacing rather than appending a list:
    /// a tag removed from the managed set is gone from the sidecar after the
    /// next edit, no matter how many edits came before it.
    ///
    /// A writer that appended instead would leave the seed's two tags in the
    /// bag beside the new one, growing it on every edit.
    #[test]
    fn a_rewritten_tag_set_carries_no_tag_from_the_previous_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sidecar = seed_sidecar_in(dir.path());

        for round in 1..=3 {
            write_xmp_properties(
                &sidecar,
                &[XmpAssignment {
                    tag: "XMP-dc:Subject",
                    value: Some(&format!("tag-{round}")),
                }],
            )
            .expect("the write should succeed");
            assert_eq!(
                xmp_properties_of(&sidecar).get("XMP-dc:Subject"),
                Some(&format!("tag-{round}").as_str().to_string()),
                "round {round}: the bag must be exactly the managed tag set"
            );
        }
    }

    /// Every way a managed field can be *absent*, and that removing it removes
    /// the property rather than blanking it.
    ///
    /// A blank is not a removal: a `dc:description` whose `x-default` entry is
    /// empty reads back as an empty description, which the app stores as a
    /// description the user never wrote. `None` is the only spelling that
    /// deletes, so this is what pins that the caller reaches for it.
    #[test]
    fn removing_a_managed_property_removes_it_rather_than_blanking_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sidecar = seed_sidecar_in(dir.path());

        for tag in ["XMP-dc:Subject", "XMP-dc:Description", "XMP-xmp:Rating"] {
            write_xmp_properties(&sidecar, &[XmpAssignment { tag, value: None }])
                .unwrap_or_else(|err| panic!("removing {tag} should succeed: {err:#}"));
            assert_eq!(
                xmp_properties_of(&sidecar).get(tag),
                None,
                "{tag} must be absent after a removal, not present and empty"
            );
        }
    }

    /// A value `ExifTool` cannot take as an argument, and the same value through
    /// the staged file that is the only way to write it.
    ///
    /// The property is a description, because `sanitize_text` keeps newlines —
    /// a multi-line description is ordinary user input, and the argument file is
    /// line-oriented. The round trip has to be exact, newlines and trailing
    /// newline included: a value that came back shortened would silently edit
    /// the user's text.
    #[test]
    fn a_value_containing_a_newline_round_trips_through_a_staged_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sidecar = seed_sidecar_in(dir.path());
        let value = "first line\nsecond line with \"quotes\" & <angle> \\ backslash\n\n";

        write_xmp_properties(
            &sidecar,
            &[XmpAssignment {
                tag: "XMP-dc:Description",
                value: Some(value),
            }],
        )
        .expect("a multi-line value must still be writable");

        assert_eq!(
            xmp_properties_of(&sidecar)
                .get("XMP-dc:Description")
                .map(String::as_str),
            Some(value),
            "the staged value must arrive whole"
        );
    }

    /// A value whose argument form would be cut in half is written through a
    /// file instead — asserted on the arguments rather than on the round trip,
    /// because the round trip alone cannot tell a staged write from a
    /// correctly-serialised inline one.
    #[test]
    fn a_value_with_a_line_break_is_staged_rather_than_passed_inline() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("photo.xmp");
        let staged = staged_value_path(&target, 0);

        let inline = super::StagedValues::new(
            &target,
            &[XmpAssignment {
                tag: "XMP-dc:Description",
                value: Some("one line"),
            }],
        )
        .expect("an inline value needs no staging");
        assert_eq!(inline.arguments(), ["-XMP-dc:Description=one line"]);

        let through_file = super::StagedValues::new(
            &target,
            &[XmpAssignment {
                tag: "XMP-dc:Description",
                value: Some("two\nlines"),
            }],
        )
        .expect("a value with a line break is staged, not refused");
        assert_eq!(
            through_file.arguments(),
            [format!("-XMP-dc:Description<={}", staged.to_string_lossy())]
        );
        // The staged file exists while the assignment is live and is gone once
        // it is not, which is what keeps an index from later finding a stray
        // file beside the media.
        assert!(staged.exists(), "the value must be written to be assigned");
        drop(through_file);
        assert!(
            !staged.exists(),
            "the value file must not outlive the write"
        );
    }

    /// The outcomes a write reports, each from a file that produces it.
    ///
    /// `Rejected` is the one that carries a remedy, so it is the one that must
    /// be right: the file is unchanged, which is what lets a caller decide the
    /// packet was unreadable rather than the write having failed.
    #[test]
    fn a_write_reports_whether_it_updated_created_or_was_refused() {
        let dir = tempfile::tempdir().expect("temp dir");

        // An absent file: a packet had to be created.
        let absent = dir.path().join("absent.xmp");
        assert_eq!(
            write_xmp_properties(
                &absent,
                &[XmpAssignment {
                    tag: "XMP-dc:Subject",
                    value: Some("only"),
                }]
            )
            .expect("writing an absent file creates it"),
            XmpWriteOutcome::Created
        );
        assert_eq!(
            xmp_properties_of(&absent).get("XMP-dc:Subject"),
            Some(&"only".to_string())
        );

        // An existing packet: updated in place.
        let existing = seed_sidecar_in(dir.path());
        assert_eq!(
            write_xmp_properties(
                &existing,
                &[XmpAssignment {
                    tag: "XMP:Rating",
                    value: Some("3"),
                }]
            )
            .expect("writing an existing packet updates it"),
            XmpWriteOutcome::Updated
        );

        // Structurally broken XMP: refused, and refused *without* damage. The
        // `rdf:Bag` is never closed, so the packet cannot be parsed and there
        // is no packet to write into.
        let broken = dir.path().join("broken.xmp");
        let broken_before = [
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>"#,
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">"#,
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#,
            r#"<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:subject><rdf:Bag><rdf:li>doomed</rdf:li>"#,
            r#"</rdf:Description></rdf:RDF></x:xmpmeta>"#,
            r#"<?xpacket end="w"?>"#,
            "",
        ]
        .join("\n");
        std::fs::write(&broken, &broken_before).expect("write the broken packet");
        assert_eq!(
            write_xmp_properties(
                &broken,
                &[XmpAssignment {
                    tag: "XMP-dc:Subject",
                    value: Some("replacement"),
                }]
            )
            .expect("a refused write is an outcome, not an error"),
            XmpWriteOutcome::Rejected
        );
        assert_eq!(
            std::fs::read_to_string(&broken).expect("the file is still there"),
            broken_before,
            "a refused write must leave the file byte-identical"
        );
    }

    /// The stdout lines each outcome is read off, as the child prints them.
    ///
    /// The strings are ExifTool's, not this code's, so they are pinned here
    /// rather than left to a change in the reader: a wording change upstream
    /// would otherwise turn every write into a silent [`XmpWriteOutcome::Updated`],
    /// which is the one misreading that loses data quietly.
    #[test]
    fn the_write_outcome_is_read_off_the_lines_exiftool_prints() {
        for (stdout, expected) in [
            ("    1 image files updated", XmpWriteOutcome::Updated),
            // A property cleared on a file that does not have it: nothing to
            // change, and *not* a refusal.
            (
                "    0 image files updated\n    1 image files unchanged",
                XmpWriteOutcome::Updated,
            ),
            ("    1 image files created", XmpWriteOutcome::Created),
            (
                "    0 image files updated\n    1 files weren't updated due to errors",
                XmpWriteOutcome::Rejected,
            ),
        ] {
            assert_eq!(
                classify_write(stdout),
                expected,
                "{stdout:?} must classify as {expected:?}"
            );
        }
        // The refusal line is what a refusal is recognised by, and no success
        // line contains it — which is what keeps a successful write from being
        // misread as a refusal.
        assert_eq!(
            classify_write("    1 image files updated"),
            XmpWriteOutcome::Updated
        );
        assert!(!"    1 image files updated".contains(WRITE_REFUSED));
    }

    /// One write per sidecar edit costs one `exiftool` child, not one per edit.
    ///
    /// `#[ignore]`d because it is a measurement, not a check — the number is the
    /// evidence for routing writes through the shared session rather than
    /// spawning per write, and it goes stale silently otherwise. Run with
    /// `cargo test -p picasu --lib -- --ignored --nocapture
    /// measures_the_sidecar_write_cost`.
    #[test]
    #[ignore = "prints timings on this machine; not an assertion"]
    fn measures_the_sidecar_write_cost() {
        const CALLS: usize = 50;
        let dir = tempfile::tempdir().expect("temp dir");
        let sidecar = dir.path().join("photo.xmp");

        let cold = Instant::now();
        write_xmp_properties(
            &sidecar,
            &[XmpAssignment {
                tag: "XMP-dc:Subject",
                value: Some("cold"),
            }],
        )
        .expect("the first write starts the session");
        let first = cold.elapsed();

        let warm = Instant::now();
        for round in 0..CALLS {
            write_xmp_properties(
                &sidecar,
                &[
                    XmpAssignment {
                        tag: "XMP-dc:Subject",
                        value: Some(&format!("tag-{round}")),
                    },
                    XmpAssignment {
                        tag: "XMP-dc:Description",
                        value: Some("a description"),
                    },
                    XmpAssignment {
                        tag: "XMP:Rating",
                        value: Some("3"),
                    },
                ],
            )
            .expect("a warm write");
        }
        let warm = warm.elapsed();

        println!(
            "first write (session start): {:.1} ms",
            first.as_secs_f64() * 1000.0
        );
        println!(
            "warm write: {:.2} ms/write over {CALLS} writes (3 properties each)",
            warm.as_secs_f64() * 1000.0 / CALLS as f64
        );
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
    /// rest of the file stays a decodable image. The TIFF byte-order word that
    /// follows the `Exif\0\0` header is corrupted in place, so the segment
    /// length and everything after it are untouched.
    ///
    /// The damage has to be one ExifTool cannot work around. It recovers from a
    /// damaged *IFD offset* — patching the last byte of the byte-order word
    /// (measured with exiftool 13.59) still yields every tag, because the
    /// directory is found by scanning — so a corrupted offset would fill this map
    /// rather than empty it. An invalid byte-order marker has no such fallback:
    /// ExifTool reports `Malformed APP1 EXIF segment` and extracts nothing.
    ///
    /// The unpatched control is asserted first, so a fixture that carries no
    /// EXIF in the first place cannot make this test pass for the wrong reason.
    #[test]
    fn a_corrupt_exif_block_yields_an_empty_map_while_the_image_stays_valid() {
        let dir = tempfile::tempdir().expect("temp dir");
        let good = dir.path().join("good.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(good.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: Some(vec!["exif_fallback_control".into()]),
            exif_date: Some("2023:07:15 10:00:00".into()),
            further_iptc: None,
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
        corrupt_exif_byte_order(&mut bytes);
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

    /// The JPEG EXIF header, the six bytes `Exif\0\0` an APP1 segment's EXIF
    /// payload starts with.
    const EXIF_HEADER: &[u8] = super::EXIF_HEADER;

    /// The corruption above is the one that empties the map, and it has to be
    /// the *byte-order* word: ExifTool recovers from a damaged IFD offset by
    /// scanning for the directory, so a test that patched the wrong four bytes
    /// would read every tag and pass the wrong way round.
    #[test]
    fn the_corrupt_exif_damage_is_load_bearing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let good = dir.path().join("good.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(good.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: Some(vec!["load_bearing".into()]),
            exif_date: Some("2023:07:15 10:00:00".into()),
            further_iptc: None,
            minimal: false,
        }])
        .expect("generate photo");

        let control = std::fs::read(&good).expect("read photo");
        assert!(
            !exif_vec_of(&good).is_empty(),
            "the fixture must carry a readable EXIF block, or the comparison below is vacuous"
        );

        // The byte order this fixture happens to carry, damaged: nothing survives.
        let mut damaged = control.clone();
        corrupt_exif_byte_order(&mut damaged);
        assert_eq!(
            damaged.len(),
            control.len(),
            "the patch must not change the length"
        );
        let empty = dir.path().join("damaged.jpg");
        std::fs::write(&empty, &damaged).expect("write photo");
        assert!(
            exif_vec_of(&empty).is_empty(),
            "an invalid byte-order word must cost every EXIF field"
        );

        // The last byte of the same word, which for a big-endian block is the
        // high byte of the IFD offset: ExifTool scans past it and reports every
        // tag, so a mutation that patched these bytes instead would leave this
        // map full and prove nothing.
        let word_at = control
            .windows(EXIF_HEADER.len())
            .position(|window| window == EXIF_HEADER)
            .expect("the fixture carries an Exif\\0\\0 header")
            + EXIF_HEADER.len();
        let mut offset_damaged = control.clone();
        offset_damaged[word_at + 3] ^= 0xFF;
        let recoverable = dir.path().join("offset-damaged.jpg");
        std::fs::write(&recoverable, &offset_damaged).expect("write photo");
        assert!(
            !exif_vec_of(&recoverable).is_empty(),
            "a damaged IFD offset is recoverable, which is why the byte-order word is \
             what the corruption test damages"
        );
    }

    /// The precondition for every EXIF scenario, and the counterpart of
    /// `process::video`'s ffmpeg check. It stays a hard failure: the pipeline
    /// propagates a toolchain failure, so a missing ExifTool turns a suite full
    /// of metadata assertions into a screen of indexing errors, and the
    /// precondition turns that into one message that says what to install.
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
            further_iptc: None,
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
            further_iptc: None,
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
        // A dead child's pipes: `Io`, a disconnected stderr reader, a mutex the
        // crate's own lock left poisoned, and the documented-but-never-constructed
        // `ProcessTerminated`.
        assert!(is_transport_failure(&ExifToolError::Io(
            std::io::Error::from(std::io::ErrorKind::BrokenPipe)
        )));
        assert!(is_transport_failure(&ExifToolError::StderrDisconnected));
        assert!(is_transport_failure(&ExifToolError::ProcessTerminated));
        assert!(is_transport_failure(&ExifToolError::MutexPoison(
            "session lock poisoned".to_string()
        )));
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

    /// The classification the whole missing-`exiftool` contract rests on, as a
    /// table over every variant the crate can produce.
    ///
    /// Two things are asserted per variant rather than one: which side of the
    /// split it falls on, and — for the toolchain side — that a failure carrying
    /// it names the install remedy. The second assertion is not decoration: a
    /// propagated failure only helps an operator if the message tells them what
    /// to do, and the remedy is the entire actionable content of the change.
    ///
    /// Every variant is listed rather than sampled, including the ones the read
    /// path cannot currently construct, because a variant added to the crate in a
    /// dependency bump has to land in one arm or the other deliberately — the
    /// default arm of [`is_toolchain_variant`] is a bet about a variant this
    /// code has never seen, and this test is what keeps that bet visible.
    #[test]
    fn every_engine_failure_is_classified_and_a_toolchain_failure_names_the_remedy() {
        for (label, err) in [
            // ── toolchain: the deployment, not the file ────────────────────
            (
                "ExifToolNotFound",
                ExifToolError::ExifToolNotFound(std::io::Error::from(std::io::ErrorKind::NotFound)),
            ),
            (
                "Io(BrokenPipe)",
                ExifToolError::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe)),
            ),
            ("StderrDisconnected", ExifToolError::StderrDisconnected),
            ("ProcessTerminated", ExifToolError::ProcessTerminated),
            (
                "MutexPoison",
                ExifToolError::MutexPoison("session lock poisoned".to_string()),
            ),
            (
                "ExifToolProcess(terminated unexpectedly)",
                ExifToolError::ExifToolProcess {
                    message: "Process terminated unexpectedly.".to_string(),
                    std_err: String::new(),
                    command_args: "-json -G1".to_string(),
                },
            ),
        ] {
            assert!(
                is_toolchain_variant(&err),
                "{label} is about the toolchain, so a caller must propagate it rather than store \
                 an asset with no metadata"
            );
            let rendered = format!("{:#}", engine_failure(err));
            for expected in [
                "exiftool",
                "just install-exiftool",
                "apt-get install libimage-exiftool-perl",
            ] {
                assert!(
                    rendered.contains(expected),
                    "{label} must arrive with a remedy, and has to mention {expected:?}: {rendered}"
                );
            }
        }

        for (label, err) in [
            // ── file: the engine ran, and this file is the reason ────────────
            (
                "ExifToolProcess(Error: Malformed APP1 EXIF segment)",
                ExifToolError::ExifToolProcess {
                    message: "Error: Malformed APP1 EXIF segment".to_string(),
                    std_err: "Error: Malformed APP1 EXIF segment".to_string(),
                    command_args: "-json -G1 broken.jpg".to_string(),
                },
            ),
            (
                "FileNotFound",
                ExifToolError::FileNotFound {
                    path: PathBuf::from("/library/absent.jpg"),
                    command_args: "-json -G1 /library/absent.jpg".to_string(),
                },
            ),
            (
                "Json",
                ExifToolError::Json(serde_json::from_str::<serde_json::Value>("{").unwrap_err()),
            ),
            (
                "Utf8",
                ExifToolError::Utf8(String::from_utf8(vec![0xff]).expect_err("invalid UTF-8")),
            ),
            (
                "UnexpectedFormat",
                ExifToolError::UnexpectedFormat {
                    path: String::new(),
                    command_args: "-json -G1".to_string(),
                },
            ),
            (
                "Deserialization",
                ExifToolError::Deserialization {
                    path: String::new(),
                    source: serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
                },
            ),
            (
                "TagNotFound",
                ExifToolError::TagNotFound {
                    path: PathBuf::from("/library/photo.jpg"),
                    tag: "NoSuchTag".to_string(),
                },
            ),
            (
                "TagDeserialization",
                ExifToolError::TagDeserialization {
                    path: PathBuf::from("/library/photo.jpg"),
                    tag: "NoSuchTag".to_string(),
                    error: serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
                },
            ),
        ] {
            assert!(
                !is_toolchain_variant(&err),
                "{label} is about this file, so the file loses its metadata and nothing else"
            );
            let rendered = format!("{:#}", engine_failure(err));
            assert!(
                !rendered.contains("install-exiftool"),
                "{label} is not an installation problem, so it must not send an operator to \
                 install a binary they already have: {rendered}"
            );
        }
    }

    /// The classification has to survive the context layers, because the caller
    /// sees the outermost error and cannot reach the crate's error itself.
    ///
    /// This is the shape a real failure arrives in: the file on the outside from
    /// `read_metadata_record`, the remedy in the middle, the crate's typed error
    /// at the bottom. A predicate that only inspected the top would classify every
    /// toolchain failure as a per-file one and the whole change would be inert.
    #[test]
    fn a_toolchain_failure_is_recognised_through_the_context_layers() {
        let toolchain = with_read_seam(
            ReadSeam::Executable(PathBuf::from("/nonexistent/picasu-no-such-exiftool")),
            || read_metadata_record(&PathBuf::from("/library/photo.jpg")).unwrap_err(),
        );
        assert!(
            is_toolchain_failure(&toolchain),
            "a session that cannot start is a toolchain failure: {toolchain:#}"
        );

        // The file-shaped failure, through the same layers, is not.
        let file_shaped = with_read_seam(
            ReadSeam::Injected(Err(ExifToolError::ExifToolProcess {
                message: "Error: Malformed APP1 EXIF segment".to_string(),
                std_err: "Error: Malformed APP1 EXIF segment".to_string(),
                command_args: "-json -G1 broken.jpg".to_string(),
            })),
            || read_metadata_record(&PathBuf::from("/library/broken.jpg")).unwrap_err(),
        );
        assert!(
            !is_toolchain_failure(&file_shaped),
            "a file ExifTool rejected is not a deployment problem: {file_shaped:#}"
        );
    }

    /// A child that dies and stays dead is the second toolchain failure, and it
    /// is the one the read layer has to decorate: `read_retrying` restarts the
    /// child once, and if the replacement dies too the failure is what a caller
    /// sees. So this asserts the remedy survives the whole retry path, not only
    /// the session-start one.
    ///
    /// The dead child is a stub script rather than a SIGKILL. The crate writes
    /// the read into the child's stdin and waits for its `{ready}` marker on
    /// stdout, so a child that exits without writing one is indistinguishable
    /// from a killed `exiftool` — and unlike a kill, it happens on the *retried*
    /// read too, which a same-thread test cannot otherwise arrange (the restart
    /// happens inside the call being made).
    #[cfg(unix)]
    #[test]
    fn a_child_that_dies_on_both_attempts_propagates_with_the_remedy() {
        let dir = tempfile::tempdir().expect("temp dir");
        let stub = dir.path().join("exiftool-that-exits");
        std::fs::write(&stub, "#!/bin/sh\nexit 0\n").expect("write the stub");
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
            .expect("make the stub executable");

        let err = with_read_seam(ReadSeam::Executable(stub), || {
            read_metadata_record(&pinned_fixture("tiff-48x32-exif"))
                .expect_err("a child that exits without answering cannot serve the read")
        });

        assert!(
            is_toolchain_failure(&err),
            "an engine whose child will not stay up is a deployment problem: {err:#}"
        );
        assert!(
            err.chain()
                .find_map(|cause| cause.downcast_ref::<ExifToolError>())
                .is_some_and(is_transport_failure),
            "and the crate's own error must be the transport one, not a file rejection: {err:#}"
        );
        let rendered = format!("{err:#}");
        for expected in [
            "exiftool",
            "just install-exiftool",
            "apt-get install libimage-exiftool-perl",
        ] {
            assert!(
                rendered.contains(expected),
                "the retried read's failure has to be actionable too, and has to mention \
                 {expected:?}: {rendered}"
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
            further_iptc: None,
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

    /// A missing binary has to stay a *named* failure, not a generic one, and it
    /// has to stay typed: the pipeline decides between "this deployment is
    /// broken" and "this file is unreadable" by downcasting, so an error that
    /// arrived as a plain string would silently downgrade a library-wide failure
    /// to a per-file one. The rendered message is the operator half of the same
    /// contract and is asserted for the remedy.
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
        let spawned = wait_for_new_exiftool_children(&before);
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
    /// sessions, minus the ones in `before`.
    ///
    /// Children are tracked per thread by the kernel, and a session is created
    /// on the thread that reads with it, so this sees one session's child and
    /// not the ones the other `exiftool` tests are using in parallel.
    ///
    /// It polls rather than reading once, because the assertion is about how many
    /// children a session owns and not about how fast the kernel publishes one:
    /// a child is listed as soon as it exists, but its `cmdline` is empty until
    /// it has `exec`'d, and under load (many `exiftool` tests in parallel) a
    /// single read can land inside that window and see nothing. Bounded at ~1 s,
    /// which is two orders of magnitude longer than the window has ever been
    /// observed to last; a child that never appears is a real failure and falls
    /// through as the empty list the assertion reports.
    #[cfg(target_os = "linux")]
    fn wait_for_new_exiftool_children(before: &[u32]) -> Vec<u32> {
        for _ in 0..100 {
            let spawned: Vec<u32> = exiftool_children_of_this_thread()
                .into_iter()
                .filter(|pid| !before.contains(pid))
                .collect();
            if !spawned.is_empty() {
                return spawned;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Vec::new()
    }

    /// PIDs of this thread's direct children that are `exiftool -stay_open`
    /// sessions.
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
