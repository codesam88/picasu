use crate::error::{AppError, ErrorKind, ResultExt};
use crate::openapi_components::Unauthorized;
use crate::process::dir_album::get_dir_path_for_album;
use crate::process::format::{self, Detection, kind_for_extension};
use crate::process::sanitize::{FilenameSanitize, find_unique_path, sanitize_filename};
use crate::router::auth::GuardMutate;
use crate::router::auth::GuardUser;
use crate::router::put::assign_album::OnConflict;
use crate::router::{AppResult, GuardResult};
use crate::storage::files::get_resolved_image_home;
use anyhow::Result;
use arrayvec::ArrayString;
use rocket::form::{Errors, Form};
use rocket::fs::TempFile;
use rocket::serde::json::Json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task::spawn_blocking;

/// Data structure representing the multipart form for a single file upload.
#[derive(FromForm, Debug)]
pub struct UploadForm<'r> {
    /// The uploaded file.
    #[field(name = "file")]
    pub file: TempFile<'r>,

    /// Timestamp (Unix epoch in milliseconds) for the uploaded file.
    #[field(name = "lastModified")]
    pub last_modified: u64,
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

    let upload_folder = crate::model::config::with_config(|c| c.upload_folder.clone());
    let target_dir = image_root.join(upload_folder);

    std::fs::create_dir_all(&target_dir).map_err(|e| {
        AppError::new(
            ErrorKind::IO,
            format!("Failed to create upload ingress folder: {e}"),
        )
    })?;

    Ok(target_dir)
}

/// Upload a single media file into an album or the upload folder and store it.
///
/// With a target album the file lands in that album's own directory; without
/// one it lands in the configured `uploadFolder` below `imagePath`, which
/// becomes its own top-level album. The file is written to its final location
/// and a 200 response returns the stored name plus the target directory
/// (`{"files": [...], "dir": ...}`) so the client can poll for exactly what
/// landed; indexing happens asynchronously in the background, so a row may
/// not be visible immediately.
///
/// Corner cases: Credentials may be admin or share — a share is accepted when
/// it sets `showUpload`, is unexpired, matches `x-share-password` and names
/// the same album as `presigned_album_id_opt`. `on_conflict` defaults to
/// `rename`, which suffixes a collision `-001`, `-002`, …; `skip` discards the
/// file without writing or indexing it, and any other value is a 400. The
/// single uploaded file is validated before it is written, so a rejected name,
/// an extension outside the image or video whitelist, or unidentifiable bytes
/// abort the request with a 400 and leave nothing behind. The stored extension
/// always comes from the part's `Content-Type` rather than the client filename.
/// Content that passes the cheap pre-flight checks but is later rejected by the
/// full decode pipeline is not removed and does not fail the request: it is
/// reported as a failed scan later. Each request stands alone: any file that is
/// stored is a success, and a failure applies only to that request.
///
/// Errors: 400 unknown `on_conflict`, unsupported or mismatched file type,
/// rejected filename, missing `imagePath` — 401 missing or invalid session —
/// 405 read-only mode — 500 storage failure.
#[utoipa::path(
        tag = "upload",
        request_body(content_type = "multipart/form-data", content = Object),
        params(
            ("presigned_album_id_opt" = Option<String>, Query, description = "Target album id (at most 64 bytes); without it files land in the configured upload folder"),
            ("on_conflict" = Option<String>, Query, description = "Conflict policy for an existing target file: rename (default; suffixes -001, -002, …) or skip; any other value is a 400"),
            ("auto_rename" = Option<bool>, Query, description = "When true (the default), uploaded filenames are sanitized automatically: forbidden characters are stripped, reserved Windows names are prefixed, and Unicode NFC normalization is applied; a name that degrades to empty falls back to 'upload', yielding an 'upload-{uuid}.{ext}' final name. When false, any file whose name cannot be kept as-is is rejected with a 400 error."),
        ),
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardUser", "GuardMutate"],
            "credentials": ["session"],
            "admin": "any",
            "shareScope": "n-a",
            "effects": ["write"],
            "invariants": ["AUTH-12"]
        }))),
        responses(
            (status = 200, description = "Upload successful", body = serde_json::Value),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post(
    "/post/upload?<presigned_album_id_opt>&<on_conflict>&<auto_rename>",
    data = "<form>"
)]
#[allow(clippy::too_many_lines)]
pub async fn upload(
    auth: GuardResult<GuardUser>,
    mutation: GuardResult<GuardMutate>,
    presigned_album_id_opt: Option<String>,
    on_conflict: Option<String>,
    auto_rename: Option<bool>,
    form: Result<Form<UploadForm<'_>>, Errors<'_>>,
) -> AppResult<Json<serde_json::Value>> {
    let GuardUser { claims } = auth?;
    let _ = mutation?;
    let (caller, _) = crate::router::put::users_password::caller_identity(&claims);

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

    let target_dir = resolve_upload_target_dir(album_id)?;

    // Write gate: target directory must be writable by caller.
    crate::storage::files::write_gate(&caller, None, Some(&target_dir))
        .map_err(|_| AppError::new(ErrorKind::PermissionDenied, "Upload target not writable"))?;

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

    let auto_rename = auto_rename.unwrap_or(true);

    // Validate the single file before writing: rejected name, unsupported or
    // mismatched type, or unidentifiable bytes abort the request with a 400
    // before anything is written.
    validate_upload_file(&inner_form.file, auto_rename, &policy)?;

    let last_modified =
        resolve_upload_timestamp(inner_form.last_modified, policy.use_client_timestamp);
    let raw_filename = get_filename(&inner_form.file);
    let filename = resolve_filename(&raw_filename, auto_rename, policy.normalize_nfc)?;
    let extension = get_extension(&inner_form.file)?;

    let Some(final_path) = save_file(
        &mut inner_form.file,
        &target_dir,
        filename,
        extension,
        last_modified,
        on_conflict_strategy.unwrap_or(OnConflict::Rename),
    )
    .await?
    else {
        // skip: dest already exists, nothing stored/indexed
        let target_dir_str = target_dir.to_string_lossy().into_owned();
        return Ok(Json(
            serde_json::json!({"files": [], "dir": target_dir_str}),
        ));
    };

    // Stored, not indexed: enqueue background indexing via the watcher
    // intake lane. Fire-and-forget; a redundant queued item is superseded
    // cheaply and a missed submit would leave the file unindexed.
    //
    // The response carries the stored name (sanitize/rename may have
    // changed it) and the target directory so the client can poll for
    // exactly what landed instead of scanning the whole library.
    let target_dir_str = target_dir.to_string_lossy().into_owned();
    submit_uploaded_path(Path::new(&final_path)).await;

    let stored_name = Path::new(&final_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Json(
        serde_json::json!({"files": [stored_name], "dir": target_dir_str}),
    ))
}

/// Validates a single uploaded file before it is written.
///
/// Rejects files whose name cannot be sanitized, whose extension is not a
/// supported image/video type, or whose content cannot be identified at all.
///
/// When the content *is* identified but contradicts the extension-derived type,
/// that is the one case `validate_content` governs: rejecting is the default,
/// and disabling the setting tolerates a mislabeled file.
fn validate_upload_file(
    file: &TempFile<'_>,
    auto_rename: bool,
    policy: &UploadPolicy,
) -> Result<(), AppError> {
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

    let tmp_path = target_dir.join(crate::process::atomic_write::temp_file_name(
        "upload", &filename,
    ));

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
    crate::model::config::with_config(|config| UploadPolicy {
        normalize_nfc: config.normalize_upload_filenames,
        validate_content: config.validate_upload_content,
        use_client_timestamp: config.use_client_timestamp_info,
    })
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

/// Enqueue a stored upload for background indexing.
///
/// Uses the watcher intake lane so dedup and supersession apply uniformly
/// regardless of watcher state. Fire-and-forget: submission failures are
/// logged, not propagated, because the file is already stored and a later
/// scan will pick it up.
pub(crate) async fn submit_uploaded_path(path: &Path) {
    if let Err(err) = crate::tasks::queue::submit_watcher_index(path.to_path_buf()).await {
        warn!("Failed to enqueue uploaded file {path:?} for indexing: {err}");
    }
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
        assert_eq!(
            resolve_filename("..jpg", true, true).expect("\"..jpg\" resolves with auto-rename"),
            "."
        );
    }

    #[test]
    fn pin_triple_dot_extension_stem() {
        // Path::file_stem("...jpg") is "..". As above, save_file appends the
        // extension, so the final name is a literal "...jpg", not "..".
        assert_eq!(
            resolve_filename("...jpg", true, true).expect("\"...jpg\" resolves with auto-rename"),
            ".."
        );
    }

    #[test]
    fn pin_leading_dot_filename() {
        // A leading dot is not an extension separator: file_stem is the full
        // name, so ".jpg" survives as-is.
        assert_eq!(
            resolve_filename(".jpg", true, true)
                .expect("leading dot name resolves with auto-rename"),
            ".jpg"
        );
    }

    #[test]
    fn pin_trailing_dot_stem() {
        assert_eq!(
            resolve_filename("foo.", true, true)
                .expect("trailing dot name resolves with auto-rename"),
            "foo"
        );
    }

    #[test]
    fn auto_rename_false_rejects_forbidden_chars() {
        let err = resolve_filename("a/b.jpg", false, false)
            .expect_err("path separator is rejected without auto-rename");
        assert!(
            err.message.contains("forbidden"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn auto_rename_false_rejects_reserved_windows_name() {
        let err = resolve_filename("con.jpg", false, false)
            .expect_err("reserved Windows name is rejected without auto-rename");
        assert!(
            err.message.contains("reserved Windows"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn auto_rename_false_rejects_unicode_normalization() {
        let err = resolve_filename("cafe\u{0301}.jpg", false, true)
            .expect_err("non-NFC name is rejected without auto-rename");
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
        assert_eq!(
            resolve_filename("a\nb.jpg", true, false)
                .expect("control characters resolve with auto-rename"),
            "ab"
        );
    }

    #[test]
    fn control_characters_reject_when_auto_rename_false() {
        let err = resolve_filename("a\nb.jpg", false, false)
            .expect_err("control characters are rejected without auto-rename");
        assert!(
            err.message.contains("forbidden"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn separator_homoglyph_rejects_when_auto_rename_false() {
        let err = resolve_filename("photo\u{FF0F}cat.jpg", false, false)
            .expect_err("homoglyph separator is rejected without auto-rename");
        assert!(
            err.message.contains("forbidden"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn separator_homoglyph_strips_when_auto_rename_true() {
        assert_eq!(
            resolve_filename("photo\u{FF0F}cat.jpg", true, false)
                .expect("homoglyph separator resolves with auto-rename"),
            "photocat"
        );
    }
}

#[cfg(test)]
mod upload_enqueue_seam_tests {
    use super::submit_uploaded_path;
    use std::path::PathBuf;

    #[test]
    fn submit_uploaded_path_bumps_path_generation() {
        // Contract: every successfully stored upload is submitted for
        // background indexing through the watcher intake lane, which bumps
        // the path's generation. No timing assertions: the bump is synchronous
        // with submission.
        let path = PathBuf::from("/upload-seam-test/upload-seam-test.jpg");
        let before = crate::tasks::path_epoch::current_generation(&path);
        let runtime = &crate::tasks::runtime::RUNTIME;
        runtime.block_on(submit_uploaded_path(&path));
        let after = crate::tasks::path_epoch::current_generation(&path)
            .expect("submission must record a path generation");
        if let Some(previous) = before {
            assert!(
                after > previous,
                "generation must strictly increase on submit"
            );
        }
    }
}
