use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::config::APP_CONFIG;
use crate::process::dir_album::get_dir_path_for_album;
use crate::process::format::{self, Detection, kind_for_extension};
use crate::process::sanitize::{FilenameSanitize, find_unique_path, sanitize_filename};
use crate::router::auth::GuardReadOnlyMode;
use crate::router::auth::GuardUpload;
use crate::router::put::assign_album::OnConflict;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::TREE;
use crate::storage::files::get_resolved_image_home;
use anyhow::Result;
use arrayvec::ArrayString;
use redb::{ReadableDatabase, ReadableTable};
use rocket::form::{Errors, Form};
use rocket::fs::TempFile;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task::spawn_blocking;
use uuid::Uuid;

/// Data structure representing the multipart form for file uploads.
#[derive(FromForm, Debug)]
pub struct UploadForm<'r> {
    /// Sequential list of uploaded files.
    #[field(name = "file")]
    pub files: Vec<TempFile<'r>>,

    /// Timestamps (Unix epoch in milliseconds) corresponding to each file by index.
    #[field(name = "lastModified")]
    pub last_modified: Vec<u64>,
}

fn get_filename(file: &TempFile<'_>) -> String {
    file.raw_name()
        .map(|n| n.dangerous_unsafe_unsanitized_raw().to_string())
        .unwrap_or_default()
}

/// Build the error shown when `auto_rename=false` and the filename needs
/// sanitization. Names the file and the specific rules that would apply.
fn auto_rename_rejected_error(raw_filename: &str, sanitize: &FilenameSanitize) -> AppError {
    let mut reasons: Vec<String> = Vec::new();
    if !sanitize.stripped.is_empty() {
        let chars: Vec<String> = sanitize.stripped.iter().map(|c| format!("{c:?}")).collect();
        reasons.push(format!("forbidden characters {}", chars.join(", ")));
    }
    if sanitize.reserved {
        reasons.push("a reserved Windows device name".to_string());
    }
    if sanitize.normalized {
        reasons.push("Unicode normalization required".to_string());
    }
    AppError::new(
        ErrorKind::InvalidInput,
        format!(
            "Filename {raw_filename:?} is not safe as-is ({}). \
             Set auto_rename=true to allow the server to rename it.",
            reasons.join("; ")
        ),
    )
}

/// Resolve the final filename for an uploaded file given the sanitizer result
/// and the `auto_rename` choice.
///
/// The returned name is the stem only -- `save_file` appends the extension
/// derived from the request `Content-Type` (the stored extension is never
/// taken from the client filename, matching pre-existing behaviour).
///
/// - `auto_rename=true` (default): use the sanitized stem; if the name
///   degrades to empty, fall back to the generated `upload` stem, which
///   `save_file` resolves to `upload.{ext}` (renamed `upload-001…` on
///   collision under the default rename strategy).
/// - `auto_rename=false`: reject with a clear message when any sanitization
///   would be needed. Tier 0 (path traversal) is never written raw.
fn resolve_filename(
    raw_filename: &str,
    auto_rename: bool,
    normalize_nfc: bool,
) -> Result<String, AppError> {
    let sanitize = sanitize_filename(raw_filename, normalize_nfc);

    let stem = if sanitize.name.is_empty() {
        String::new()
    } else {
        Path::new(&sanitize.name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };

    if auto_rename {
        if stem.is_empty() {
            // save_file appends the extension, so the stem alone yields the
            // documented "upload.{ext}" fallback.
            Ok("upload".to_string())
        } else {
            Ok(stem)
        }
    } else if sanitize.changed || sanitize.name.is_empty() {
        Err(auto_rename_rejected_error(raw_filename, &sanitize))
    } else {
        Ok(stem)
    }
}

/// Resolve where uploaded files for this request should land on disk, under
/// `IMAGE_HOME` -- there is no staging area; uploads write directly into
/// their real, final location (see `TODO.md` "Storage architecture fix").
///
/// With a target album, that's the album's own directory (resolved the same
/// way `assign_album` resolves it). With no target album, it's the
/// configured `uploadFolder` subdirectory under the resolved `imagePath`
/// (created if missing) -- it becomes its own top-level album automatically,
/// since album = directory.
fn resolve_upload_target_dir(album_id: Option<ArrayString<64>>) -> Result<PathBuf, AppError> {
    if let Some(album_id) = album_id {
        return get_dir_path_for_album(album_id)
            .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Target album not found"));
    }

    let image_root = get_resolved_image_home().ok_or_else(|| {
        AppError::new(
            ErrorKind::InvalidInput,
            "No imagePath configured -- set one in Settings before uploading without a target album",
        )
    })?;

    let upload_folder = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned")
        .upload_folder
        .clone();
    let target_dir = image_root.join(upload_folder);

    std::fs::create_dir_all(&target_dir).map_err(|e| {
        AppError::new(
            ErrorKind::IO,
            format!("Failed to create upload ingress folder: {e}"),
        )
    })?;

    Ok(target_dir)
}

#[utoipa::path(
        post,
        path = "/upload",
        request_body = Value,
        params(
            ("auto_rename" = Option<bool>, Query, description = "When true (the default), uploaded filenames are sanitized automatically: forbidden characters are stripped, reserved Windows names are prefixed, and Unicode NFC normalization is applied; a name that degrades to empty falls back to 'upload', yielding an 'upload-{uuid}.{ext}' final name. When false, any file whose name cannot be kept as-is is rejected with a 400 error."),
        ),
        responses(
            (status = 200, description = "Upload successful"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[post(
    "/upload?<presigned_album_id_opt>&<on_conflict>&<auto_rename>",
    data = "<form>"
)]
pub async fn upload(
    auth: GuardResult<GuardUpload>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    presigned_album_id_opt: Option<String>,
    on_conflict: Option<String>,
    auto_rename: Option<bool>,
    form: Result<Form<UploadForm<'_>>, Errors<'_>>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;

    let mut inner_form = match form {
        Ok(f) => f.into_inner(),
        Err(errors) => {
            // Flatten generic Rocket errors into a single context for debugging
            let error_msg = errors
                .iter()
                .fold(String::from("Form parsing failed: "), |acc, e| {
                    format!("{acc}; {e}")
                });
            return Err(AppError::new(ErrorKind::InvalidInput, error_msg));
        }
    };

    let album_id: Option<ArrayString<64>> = match presigned_album_id_opt {
        Some(s) => Some(
            ArrayString::from(&s)
                .map_err(|_| AppError::new(ErrorKind::InvalidInput, "Album ID exceeds 64 bytes"))?,
        ),
        None => None,
    };

    // Ensure strict 1:1 mapping between files and metadata
    if inner_form.files.len() != inner_form.last_modified.len() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "Mismatch between file count and timestamp count.",
        ));
    }

    let target_dir = resolve_upload_target_dir(album_id)?;

    let policy = read_upload_policy();

    // Absent `on_conflict` defaults to `rename` (base name, auto-`-001` suffix
    // on collision).
    let on_conflict_strategy: Option<OnConflict> = match on_conflict.as_deref() {
        None | Some("rename") => Some(OnConflict::Rename),
        Some("skip") => Some(OnConflict::Skip),
        Some(other) => {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                format!("Unknown on_conflict value: {other}; expected skip, or rename"),
            ));
        }
    };

    // Pre-flight: validate every file before writing any. A single bad file
    // (unsanitizable name, unknown type, content mismatch) aborts the whole
    // batch with a 400 before anything is written, so a partial upload never
    // leaves a subset of files behind.
    validate_upload_batch(&inner_form.files, auto_rename.unwrap_or(true), &policy)?;

    // Write and index each validated file.
    for (i, file) in inner_form.files.iter_mut().enumerate() {
        let last_modified =
            resolve_upload_timestamp(inner_form.last_modified[i], policy.use_client_timestamp);
        let raw_filename = get_filename(file);
        let filename = resolve_filename(
            &raw_filename,
            auto_rename.unwrap_or(true),
            policy.normalize_nfc,
        )?;
        let extension = get_extension(file)?;

        let Some(final_path) = save_file(
            file,
            &target_dir,
            filename,
            extension,
            last_modified,
            on_conflict_strategy.unwrap_or(OnConflict::Rename),
        )
        .await?
        else {
            continue; // skip: dest already exists, nothing to index
        };
        let image_root = get_resolved_image_home()
            .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "No imagePath configured"))?;
        let relative_src = Path::new(&final_path)
            .strip_prefix(&image_root)
            .map_err(|_| {
                AppError::new(ErrorKind::Internal, "Uploaded file path outside IMAGE_HOME")
            })?;
        if let Err(index_error) = crate::workflow::index_image(relative_src, None).await {
            // A mid-pipeline failure can occur after a partial commit: the
            // uploaded file is only safely removable when no index record
            // references it. Only a content-decode failure guarantees that,
            // so check before deleting — otherwise the file would be removed
            // while a committed index record still points at it.
            let outcome = classify_index_failure(
                &index_error,
                record_exists_for(Path::new(&final_path), relative_src),
            );
            error!(
                "upload of {final_path} did not index: {} ({index_error:#})",
                outcome.kind.diagnosis()
            );
            if outcome.remove_file {
                // The upload has not succeeded, and the user gets this error
                // directly in the upload response, so the never-indexed file
                // is removed as part of the failed upload action. Files are
                // never deleted after a successful upload — only by an
                // explicit user deletion action.
                if let Err(remove_error) = std::fs::remove_file(&final_path) {
                    error!("Failed to remove unindexable upload {final_path}: {remove_error}");
                }
            }
            return Err(outcome.kind.into_app_error());
        }
    }

    Ok(())
}

/// What the server concluded about an upload whose file did not index, and what
/// it does about the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UploadIndexOutcome {
    kind: UploadIndexFailure,
    /// Whether the file may be deleted.
    ///
    /// The invariant this carries: **a file in the library that no index record
    /// references is never left behind by a failed upload**, and a file a record
    /// does reference is never deleted. It answers a question about what the
    /// library holds, so it is decided by the record alone and is deliberately
    /// independent of the diagnosis — which is why it travels as a field rather
    /// than as an arm of [`UploadIndexFailure`]. The two used to be decided in
    /// two places, and that left the removal predicate unpinned: a test could
    /// only restate it. Deciding both here means one line, covered by
    /// `every_combination_has_a_diagnosis_and_a_removal`.
    remove_file: bool,
}

/// Why an uploaded file did not end up indexed, and what the client is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UploadIndexFailure {
    /// The metadata toolchain could not be read: no `exiftool`, no session, or
    /// a child that keeps dying. Nothing was learned about the bytes, so this is
    /// the server's fault and the response says how to fix it.
    ToolchainUnavailable,
    /// An index record references the path, so the file stays on disk: removing
    /// it would leave a committed record pointing at nothing.
    AlreadyIndexed,
    /// Nothing references the file and the bytes are the reason.
    Undecodable,
}

/// Decide what a failed index means for an upload: the diagnosis, and whether
/// the file may be removed.
///
/// Pure, and taking the error as an argument rather than reaching for process
/// state, so the decision is testable without a running server and without
/// breaking `exiftool` on the machine running the test. The error is the real
/// `anyhow` chain the index task returned, not a summary: the classification
/// looks for the engine's own error inside it, and a summary would have lost
/// exactly the part that matters. That is also why this cannot be a
/// resolve-the-binary-on-`PATH` check — a live child that keeps dying is a
/// toolchain failure while `exiftool` sits on `PATH`, and a resolve would report
/// that as an undecodable upload.
///
/// A toolchain failure outranks the record check for the diagnosis: whoever
/// indexed the path, the deployment is what is broken, and the operator is the
/// one who has to act on it. The record still decides the removal.
fn classify_index_failure(index_error: &anyhow::Error, record_exists: bool) -> UploadIndexOutcome {
    let kind = if crate::process::exif::is_toolchain_failure(index_error) {
        UploadIndexFailure::ToolchainUnavailable
    } else if record_exists {
        UploadIndexFailure::AlreadyIndexed
    } else {
        UploadIndexFailure::Undecodable
    };
    UploadIndexOutcome {
        kind,
        remove_file: !record_exists,
    }
}

impl UploadIndexFailure {
    /// The one-line conclusion the log records alongside the error chain.
    fn diagnosis(self) -> &'static str {
        match self {
            UploadIndexFailure::ToolchainUnavailable => {
                "the metadata toolchain could not be read, so the file's metadata is unknown"
            }
            UploadIndexFailure::AlreadyIndexed => {
                "the file was indexed but the pipeline failed, so it was kept"
            }
            UploadIndexFailure::Undecodable => {
                "nothing references the file, so it was removed as unindexable"
            }
        }
    }

    /// The response the client gets.
    ///
    /// Both pre-existing messages are reproduced byte for byte. They are the
    /// contract the upload scenarios assert and the frontend's error toast shows,
    /// and a rewording here would break both for no gain.
    fn into_app_error(self) -> AppError {
        match self {
            // 500, not 400: the client's bytes were never the problem, and a
            // client-error status tells them retrying or changing the file is the
            // fix when it is not. The message is the same sentence the log and
            // the index failure carry, so the operator is told the same thing
            // whichever they read.
            //
            // The file is still removed when nothing references it, exactly as
            // for a decode failure. The invariant is "no unindexed file is left
            // in the library", and a deployment fault is no reason to break it:
            // the client is told explicitly, so nothing about the upload is
            // ambiguous, and the bytes are still wherever the client keeps them.
            // Keeping the file instead would put an unindexed file in the
            // library that a later scan would then index — with the same empty
            // metadata this change exists to prevent.
            UploadIndexFailure::ToolchainUnavailable => AppError::new(
                ErrorKind::Internal,
                crate::process::exif::toolchain_diagnostic(),
            ),
            UploadIndexFailure::AlreadyIndexed => {
                AppError::new(ErrorKind::Internal, "Upload failed during indexing")
            }
            UploadIndexFailure::Undecodable => AppError::new(
                ErrorKind::InvalidInput,
                "Uploaded file could not be decoded as an image or video",
            ),
        }
    }
}

/// Validates every file in the batch before any file is written.
///
/// Rejects files whose name cannot be sanitized, whose extension is not a
/// supported image/video type, or whose content cannot be identified at all.
///
/// When the content *is* identified but contradicts the extension-derived type,
/// that is the one case `validate_content` governs: rejecting is the default,
/// and disabling the setting tolerates a mislabeled file. Any rejection aborts
/// the whole request, so a partial upload never leaves a subset of files behind.
fn validate_upload_batch(
    files: &[TempFile<'_>],
    auto_rename: bool,
    policy: &UploadPolicy,
) -> Result<(), AppError> {
    for file in files {
        let raw_filename = get_filename(file);
        resolve_filename(&raw_filename, auto_rename, policy.normalize_nfc)?;
        let extension = get_extension(file)?;

        if kind_for_extension(&extension).is_none() {
            error!("Rejected invalid file type: {}", extension);
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                format!("Invalid file type: {extension}"),
            ));
        }

        match detect_upload_content(file)? {
            // Nothing could identify the bytes, so there is no way to tell
            // whether they are what the extension claims. Always rejected.
            None => {
                error!("Rejected unrecognized upload content: {raw_filename:?}");
                return Err(unrecognized_upload_content(&raw_filename, &extension));
            }
            // Identified as a real format that is outside our table, so it
            // cannot match the extension-derived type. Same corner case as a
            // plain mismatch, reported with the format that was found.
            Some(Detection::Unsupported { detected_extension }) => {
                if policy.validate_content {
                    error!(
                        "Rejected unsupported upload content {raw_filename:?}: detected {detected_extension}"
                    );
                    return Err(unsupported_upload_content(
                        &raw_filename,
                        &detected_extension,
                    ));
                }
            }
            Some(Detection::Supported(detected))
                if !format::extension_matches(&extension, detected) =>
            {
                if policy.validate_content {
                    error!(
                        "Rejected misnamed upload {raw_filename:?}: declared {extension}, detected {}",
                        detected.canonical_extension()
                    );
                    return Err(mismatched_upload_content(
                        &raw_filename,
                        &extension,
                        detected.canonical_extension(),
                    ));
                }
            }
            Some(Detection::Supported(_)) => {}
        }
    }
    Ok(())
}

/// Persists the temporary file directly into `target_dir` (its real, final
/// location under `IMAGE_HOME`) with the correct modification time.
///
/// The original filename is used and the conflict strategy applied. On a
/// collision at the final path, `Rename` auto-suffixes a unique `-NNN` variant;
/// `Skip` returns `None` without writing. Neither mode overwrites an existing
/// file.
///
/// Returns `Some(path)` on success, `None` if skip left the destination intact.
async fn save_file(
    file: &mut TempFile<'_>,
    target_dir: &Path,
    filename: String,
    extension: String,
    last_modified_ms: u64,
    on_conflict: OnConflict,
) -> Result<Option<String>, AppError> {
    let target_dir = target_dir.to_path_buf();

    let tmp_path = target_dir.join(format!("{filename}-{}.tmp", Uuid::new_v4()));

    // Move to a temp location first to avoid blocking the async runtime with IO.
    // The watcher ignores ".tmp" (not a recognised media extension), so this
    // is safe even though target_dir is itself inside the watched tree.
    file.move_copy_to(&tmp_path)
        .await
        .or_raise(|| (ErrorKind::IO, "Failed to move temporary file"))?;

    let filename_owned = filename.clone();
    let tmp_path_owned = tmp_path.clone();

    // Perform metadata operations and rename in a blocking thread.
    // 1. Set mtime on the .tmp file.
    // 2. Atomic rename to .ext (final state).
    // This ensures the file watcher only picks up the file once it is fully
    // written and has the correct timestamp.
    let result = spawn_blocking(move || -> Result<Option<String>, AppError> {
        let base_final = target_dir.join(format!("{filename_owned}.{extension}"));

        let final_path = if base_final.exists() {
            match on_conflict {
                OnConflict::Skip => {
                    let _ = std::fs::remove_file(&tmp_path_owned);
                    return Ok(None);
                }
                OnConflict::Rename => find_unique_path(&base_final)?,
            }
        } else {
            base_final
        };

        set_last_modified_time(&tmp_path_owned, last_modified_ms)?;
        std::fs::rename(&tmp_path_owned, &final_path)
            .or_raise(|| (ErrorKind::IO, "Failed to rename file"))?;

        Ok(Some(final_path.to_string_lossy().into_owned()))
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    Ok(result)
}

#[allow(clippy::cast_possible_wrap)]
fn set_last_modified_time(path: &Path, last_modified_ms: u64) -> Result<(), AppError> {
    let mtime = filetime::FileTime::from_unix_time((last_modified_ms / 1000) as i64, 0);
    filetime::set_file_mtime(path, mtime)
        .or_raise(|| (ErrorKind::IO, "Failed to set file modification time"))?;
    Ok(())
}

/// Resolve the modification time applied to an uploaded file.
///
/// Controlled by `use_client_timestamp_info`. When disabled (the default),
/// the client-provided `lastModified` is ignored and `now` is used, since a
/// client clock or timezone cannot be relied on. When enabled, the value is
/// trusted but clamped to `[1970-01-01, now + 24h]` so a broken value cannot
/// date an undated file (e.g. a screenshot) to 1970 or the distant future.
/// The `[1970-01-01, …]` lower bound is implicit: Unix timestamps are
/// milliseconds since the epoch, so smaller values cannot be expressed.
fn resolve_upload_timestamp(client_ms: u64, use_client_timestamp: bool) -> u64 {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let now_ms = u64::try_from(now_ms).unwrap_or(0);
    resolve_upload_timestamp_at(client_ms, use_client_timestamp, now_ms)
}

/// Core of [`resolve_upload_timestamp`] with an injectable clock for tests.
fn resolve_upload_timestamp_at(client_ms: u64, use_client_timestamp: bool, now_ms: u64) -> u64 {
    const DAY_MS: u64 = 24 * 60 * 60 * 1000;
    if !use_client_timestamp {
        return now_ms;
    }
    client_ms.clamp(0, now_ms + DAY_MS)
}

fn get_extension(file: &TempFile<'_>) -> Result<String, AppError> {
    file.content_type()
        .and_then(|ct| ct.extension())
        .map(|ext| ext.as_str().to_lowercase())
        .ok_or_else(|| {
            error!("Failed to determine file extension from Content-Type");
            AppError::new(ErrorKind::InvalidInput, "Missing or unknown file extension")
        })
}

/// Upload-related config flags, read once per request.
struct UploadPolicy {
    normalize_nfc: bool,
    validate_content: bool,
    use_client_timestamp: bool,
}

fn read_upload_policy() -> UploadPolicy {
    let config = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned");
    UploadPolicy {
        normalize_nfc: config.normalize_upload_filenames,
        validate_content: config.validate_upload_content,
        use_client_timestamp: config.use_client_timestamp_info,
    }
}

/// Whether any index record references the uploaded file as either the
/// absolute path or an `IMAGE_HOME`-relative path.
///
/// Checks the on-disk `ASSET_BY_PATH` (authoritative for a committed insert that
/// a mid-pipeline failure may leave before it reaches the in-memory tree) and
/// the in-memory snapshot. Used by the upload error path to decide whether an
/// uploaded file can be safely deleted: deleting is only safe when no record
/// references it. If the database cannot be read, defaults to `true` (keep the
/// file) so an uncertain lookup never deletes an uploaded file.
fn record_exists_for(path: &Path, relative: &Path) -> bool {
    let matches = |record_path: &str| {
        let candidate = Path::new(record_path);
        candidate == path || candidate == relative
    };

    let mem = TREE.in_memory.read().expect("lock poisoned");
    if mem
        .iter()
        .any(|dt| dt.abstract_data.path().iter().any(|a| matches(&a.file)))
    {
        return true;
    }
    drop(mem);

    if let Ok(txn) = TREE.in_disk.begin_read()
        && let Ok(table) = txn.open_table(crate::storage::db::ASSET_BY_PATH)
        && let Ok(mut iter) = table.iter()
    {
        return iter.any(|entry| entry.is_ok_and(|(key, _)| matches(key.value())));
    }

    warn!("Could not verify index records for upload {path:?}; keeping file");
    true
}

/// Detect the content format of an uploaded file.
fn detect_upload_content(file: &TempFile<'_>) -> Result<Option<Detection>, AppError> {
    let path = file
        .path()
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Uploaded file is empty"))?;
    format::detect_from_path(path)
        .or_raise(|| (ErrorKind::IO, "Failed to read uploaded file for validation"))
}

fn unrecognized_upload_content(filename: &str, extension: &str) -> AppError {
    AppError::new(
        ErrorKind::InvalidInput,
        format!("Uploaded file {filename:?} is not recognized as {extension}"),
    )
}

fn unsupported_upload_content(filename: &str, detected: &str) -> AppError {
    AppError::new(
        ErrorKind::InvalidInput,
        format!("Uploaded file {filename:?} contains {detected} content, which is not supported"),
    )
}

fn mismatched_upload_content(filename: &str, extension: &str, detected: &str) -> AppError {
    AppError::new(
        ErrorKind::InvalidInput,
        format!(
            "Uploaded file {filename:?} contains {detected} content, but the declared type is {extension}"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::resolve_upload_timestamp_at;

    // Fixed "now" so the clamp bounds are deterministic.
    const NOW_MS: u64 = 1_700_000_000_000; // 2023-11-14T22:13:20Z

    #[test]
    fn trust_clamps_far_future_to_now_plus_24h() {
        let far_future = NOW_MS + 100_000_000_000; // ~year 2100
        let upper = NOW_MS + 24 * 60 * 60 * 1000;
        assert_eq!(resolve_upload_timestamp_at(far_future, true, NOW_MS), upper);
    }

    #[test]
    fn trust_clamps_zero_to_epoch() {
        assert_eq!(resolve_upload_timestamp_at(0, true, NOW_MS), 0);
    }

    #[test]
    fn trust_preserves_in_range_values() {
        let value = NOW_MS - 7 * 24 * 60 * 60 * 1000;
        assert_eq!(resolve_upload_timestamp_at(value, true, NOW_MS), value);
    }

    #[test]
    fn trust_clamps_epoch_boundary_plus_24h() {
        let boundary = NOW_MS + 24 * 60 * 60 * 1000;
        assert_eq!(
            resolve_upload_timestamp_at(boundary, true, NOW_MS),
            boundary
        );
    }

    #[test]
    fn trust_does_not_overflow_with_max_value() {
        let upper = NOW_MS + 24 * 60 * 60 * 1000;
        assert_eq!(resolve_upload_timestamp_at(u64::MAX, true, NOW_MS), upper);
    }

    #[test]
    fn disabled_ignores_client_value_and_uses_now() {
        assert_eq!(resolve_upload_timestamp_at(0, false, NOW_MS), NOW_MS);
        assert_eq!(resolve_upload_timestamp_at(u64::MAX, false, NOW_MS), NOW_MS);
    }
}

#[cfg(test)]
mod index_failure_tests {
    use super::{AppError, ErrorKind, UploadIndexFailure, classify_index_failure};
    use crate::process::exif::is_toolchain_failure;
    use exiftool::ExifToolError;

    /// The index error a missing `exiftool` produces on the upload path, with
    /// the same context layers the real one stacks.
    ///
    /// Outermost first, as `format!("{err:#}")` renders it: the index task's
    /// context, the read's file context, the remedy, then the engine's own typed
    /// error. Built by hand rather than produced by the pipeline because the
    /// pipeline runs on an index worker thread, which a thread-local test cannot
    /// reach — the shape of the chain is what the classification consumes, and
    /// that is reproduced exactly.
    fn toolchain_index_error() -> anyhow::Error {
        let engine =
            ExifToolError::ExifToolNotFound(std::io::Error::from(std::io::ErrorKind::NotFound));
        anyhow::Error::new(engine)
            .context(crate::process::exif::toolchain_diagnostic())
            .context("failed to read metadata for /library/uploads/photo.jpeg")
            .context(
                "failed to process image metadata pipeline. Hash: abcd, Path: uploads/photo.jpeg",
            )
    }

    /// The remedy an operator has to be given, checked as substrings because the
    /// wording is documentation and the *presence* is the contract.
    fn assert_names_the_remedy(message: &str) {
        for expected in [
            "exiftool",
            "just install-exiftool",
            "apt-get install libimage-exiftool-perl",
        ] {
            assert!(
                message.contains(expected),
                "a toolchain failure must be actionable, and has to mention {expected:?}: \
                 {message}"
            );
        }
    }

    /// A deployment whose `exiftool` cannot start is a server fault, not a client
    /// one, and the response has to say so with the remedy in it.
    ///
    /// This is the whole point of the classification reaching the HTTP boundary.
    /// The bytes were fine — the client uploaded a decodable JPEG — so answering
    /// 400 "could not be decoded as an image or video" blamed the client for the
    /// server's broken install and gave the operator nothing to act on.
    ///
    /// The error is built with the same context layers the real path stacks, so
    /// the classification is exercised over the chain a caller actually receives
    /// rather than over a bare engine error.
    #[test]
    fn a_toolchain_failure_is_a_500_carrying_the_remedy() {
        let index_error = toolchain_index_error();
        // Control: this is the failure the classification is supposed to
        // recognise, so a test that "passes" because the error was never
        // toolchain-shaped is caught here.
        assert!(
            is_toolchain_failure(&index_error),
            "control: the error under test must be toolchain-classified: {index_error:#}"
        );

        let response: AppError = classify_index_failure(&index_error, false)
            .kind
            .into_app_error();

        assert_eq!(
            response.kind,
            ErrorKind::Internal,
            "a broken deployment is the server's fault, so it must not be reported as bad input"
        );
        assert_eq!(
            response.http_status(),
            rocket::http::Status::InternalServerError,
            "and the kind has to actually reach the client as a 500"
        );
        assert_names_the_remedy(&response.message);
        assert!(
            !response.message.contains("could not be decoded"),
            "the decode diagnosis is wrong here and must not be sent: {}",
            response.message
        );
    }

    /// Undecodable bytes on a healthy deployment keep the exact response they
    /// have always had: 400, the same sentence, and the same file removal. This
    /// is the branch the upload scenarios and the frontend's error toast depend
    /// on, and the toolchain case must not have moved it.
    #[test]
    fn an_undecodable_file_stays_a_400_with_the_original_message() {
        let index_error = anyhow::Error::new(ExifToolError::ExifToolProcess {
            message: "Error: Malformed APP1 EXIF segment".to_string(),
            std_err: "Error: Malformed APP1 EXIF segment".to_string(),
            command_args: "-json -G1 broken.jpg".to_string(),
        })
        .context("failed to decode image into DynamicImage");
        assert!(
            !is_toolchain_failure(&index_error),
            "control: a file ExifTool rejected is not a deployment problem"
        );

        let response = classify_index_failure(&index_error, false)
            .kind
            .into_app_error();

        assert_eq!(response.kind, ErrorKind::InvalidInput);
        assert_eq!(
            response.http_status(),
            rocket::http::Status::BadRequest,
            "the client's own file is a client error"
        );
        assert_eq!(
            response.message, "Uploaded file could not be decoded as an image or video",
            "the message is the contract the scenarios and the frontend toast assert"
        );
    }

    /// A record that already references the file keeps its own 500 when the
    /// cause is the bytes, and that is the one case where a failure must not
    /// remove anything.
    #[test]
    fn an_existing_record_keeps_its_own_500_for_a_decode_failure() {
        let decode_failure = anyhow::Error::msg("failed to decode image into DynamicImage");
        let response = classify_index_failure(&decode_failure, true)
            .kind
            .into_app_error();
        assert_eq!(response.kind, ErrorKind::Internal);
        assert_eq!(response.message, "Upload failed during indexing");
    }

    /// A toolchain failure outranks the record for the *diagnosis*, so the
    /// operator gets the remedy even when something else already indexed the
    /// path. The file still stays: removal is the handler's `!record_exists`
    /// condition, which this test pins by asserting the record case is not
    /// reported as an unindexed upload.
    #[test]
    fn a_toolchain_failure_outranks_the_record_for_the_diagnosis_only() {
        let outcome = classify_index_failure(&toolchain_index_error(), true);
        assert_eq!(
            outcome.kind,
            UploadIndexFailure::ToolchainUnavailable,
            "the deployment is what is broken, whoever indexed the path"
        );
        assert_names_the_remedy(&outcome.kind.into_app_error().message);
        assert!(
            !outcome.remove_file,
            "a record still points at the file, so the toolchain fault must not delete it"
        );
    }

    /// The classification is a total function of its two inputs, so every
    /// combination is pinned here rather than only the ones the tests above
    /// happen to produce, together with the removal the handler pairs it with.
    /// The pair is the actual contract: a diagnosis without knowing whether the
    /// file is deleted is not what a caller acts on.
    #[test]
    fn every_combination_has_a_diagnosis_and_a_removal() {
        for (label, index_error, record_exists, expected) in [
            (
                "toolchain, no record",
                toolchain_index_error(),
                false,
                (
                    UploadIndexFailure::ToolchainUnavailable,
                    ErrorKind::Internal,
                    // nothing references it, so the never-indexed file is removed
                    true,
                ),
            ),
            (
                "toolchain, record",
                toolchain_index_error(),
                true,
                (
                    UploadIndexFailure::ToolchainUnavailable,
                    ErrorKind::Internal,
                    // a record points at it, so it must stay
                    false,
                ),
            ),
            (
                "decode, no record",
                anyhow::Error::msg("failed to decode image into DynamicImage"),
                false,
                (
                    UploadIndexFailure::Undecodable,
                    ErrorKind::InvalidInput,
                    true,
                ),
            ),
            (
                "decode, record",
                anyhow::Error::msg("failed to decode image into DynamicImage"),
                true,
                (
                    UploadIndexFailure::AlreadyIndexed,
                    ErrorKind::Internal,
                    false,
                ),
            ),
        ] {
            let outcome = classify_index_failure(&index_error, record_exists);
            assert_eq!(outcome.kind, expected.0, "{label}: wrong diagnosis");
            assert_eq!(
                outcome.kind.into_app_error().kind,
                expected.1,
                "{label}: wrong status"
            );
            assert_eq!(
                outcome.remove_file, expected.2,
                "{label}: the file must be removed exactly when nothing references it, \
                 whichever failure produced it"
            );
            assert!(
                !outcome.kind.diagnosis().is_empty(),
                "{label} must produce a log conclusion"
            );
        }
    }
}

#[cfg(test)]
mod resolve_filename_tests {
    use super::resolve_filename;

    #[test]
    fn degenerate_name_falls_back_to_upload_stem() {
        let name = resolve_filename("///", true, true)
            .expect("auto_rename fallback should always succeed");
        assert_eq!(name, "upload");
    }

    #[test]
    fn pin_dotdot_extension_stem() {
        // Path::file_stem("..jpg") is "." (the last dot splits the extension),
        // not "..". save_file always appends a validated extension, so the
        // final on-disk name stays inside the album dir -- never a bare ".".
        assert_eq!(resolve_filename("..jpg", true, true).unwrap(), ".");
    }

    #[test]
    fn pin_triple_dot_extension_stem() {
        // Path::file_stem("...jpg") is "..". As above, save_file appends the
        // extension, so the final name is a literal "...jpg", not "..".
        assert_eq!(resolve_filename("...jpg", true, true).unwrap(), "..");
    }

    #[test]
    fn pin_leading_dot_filename() {
        // A leading dot is not an extension separator: file_stem is the full
        // name, so ".jpg" survives as-is.
        assert_eq!(resolve_filename(".jpg", true, true).unwrap(), ".jpg");
    }

    #[test]
    fn pin_trailing_dot_stem() {
        assert_eq!(resolve_filename("foo.", true, true).unwrap(), "foo");
    }

    #[test]
    fn auto_rename_false_rejects_forbidden_chars() {
        let err = resolve_filename("a/b.jpg", false, false).unwrap_err();
        assert!(
            err.message.contains("forbidden"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn auto_rename_false_rejects_reserved_windows_name() {
        let err = resolve_filename("con.jpg", false, false).unwrap_err();
        assert!(
            err.message.contains("reserved Windows"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn auto_rename_false_rejects_unicode_normalization() {
        let err = resolve_filename("cafe\u{0301}.jpg", false, true).unwrap_err();
        assert!(
            err.message.contains("normalization"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn auto_rename_false_accepts_safe_name() {
        let name =
            resolve_filename("photo.jpg", false, false).expect("a safe name should pass unchanged");
        assert_eq!(name, "photo");
    }

    #[test]
    fn control_characters_strip_when_auto_rename_true() {
        assert_eq!(resolve_filename("a\nb.jpg", true, false).unwrap(), "ab");
    }

    #[test]
    fn control_characters_reject_when_auto_rename_false() {
        let err = resolve_filename("a\nb.jpg", false, false).unwrap_err();
        assert!(
            err.message.contains("forbidden"),
            "unexpected message: {err}"
        );
    }
}
