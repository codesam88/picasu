use crate::constant::{VALID_IMAGE_EXTENSIONS, VALID_VIDEO_EXTENSIONS};
use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::config::APP_CONFIG;
use crate::process::dir_album::get_dir_path_for_album;
use crate::process::sanitize::{FilenameSanitize, sanitize_filename};
use crate::router::auth::GuardReadOnlyMode;
use crate::router::auth::GuardUpload;
use crate::router::put::assign_album::OnConflict;
use crate::router::{AppResult, GuardResult};
use crate::storage::files::get_resolved_image_home;
use anyhow::Result;
use arrayvec::ArrayString;
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
///   degrades to empty, fall back to the generated `upload` stem (the UUID
///   suffix added by `save_file` yields `upload-{uuid}.{ext}`).
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
            // save_file appends its own UUID when on_conflict is None, so the
            // stem alone yields the documented "upload-{uuid}.{ext}" fallback.
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

    let on_conflict_strategy: Option<OnConflict> = match on_conflict.as_deref() {
        None => None,
        Some("skip") => Some(OnConflict::Skip),
        Some("rename") => Some(OnConflict::Rename),
        Some("replace") => Some(OnConflict::Replace),
        Some(other) => {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                format!("Unknown on_conflict value: {other}; expected skip, rename, or replace"),
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
            on_conflict_strategy,
        )
        .await?
        else {
            continue; // on_conflict=skip and destination existed
        };
        let image_root = get_resolved_image_home()
            .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "No imagePath configured"))?;
        let relative_src = Path::new(&final_path)
            .strip_prefix(&image_root)
            .map_err(|_| {
                AppError::new(ErrorKind::Internal, "Uploaded file path outside IMAGE_HOME")
            })?;
        if let Err(index_error) = crate::workflow::index_image(relative_src, None).await {
            // The upload has not succeeded, and the user gets this error
            // directly in the upload response, so the never-indexed file
            // is removed as part of the failed upload action. Files are
            // never deleted after a successful upload — only by an
            // explicit user deletion action.
            error!("Uploaded file could not be decoded as an image or video: {index_error}");
            if let Err(remove_error) = std::fs::remove_file(&final_path) {
                error!("Failed to remove unindexable upload {final_path}: {remove_error}");
            }
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "Uploaded file could not be decoded as an image or video",
            ));
        }
    }

    Ok(())
}

/// Validates every file in the batch before any file is written.
///
/// Rejects files whose name cannot be sanitized, whose extension is not a
/// supported image/video type, or (when `validate_content` is enabled) whose
/// decoded content does not match the declared type. Any rejection aborts the
/// whole request, so a partial upload never leaves a subset of files behind.
fn validate_upload_batch(
    files: &[TempFile<'_>],
    auto_rename: bool,
    policy: &UploadPolicy,
) -> Result<(), AppError> {
    for file in files {
        let raw_filename = get_filename(file);
        resolve_filename(&raw_filename, auto_rename, policy.normalize_nfc)?;
        let extension = get_extension(file)?;

        if !(VALID_IMAGE_EXTENSIONS.contains(&extension.as_str())
            || VALID_VIDEO_EXTENSIONS.contains(&extension.as_str()))
        {
            error!("Rejected invalid file type: {}", extension);
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                format!("Invalid file type: {extension}"),
            ));
        }
        if policy.validate_content {
            validate_upload_content(file, &extension)?;
        }
    }
    Ok(())
}

/// Persists the temporary file directly into `target_dir` (its real, final
/// location under `IMAGE_HOME`) with the correct modification time.
///
/// When `on_conflict` is `None` (default), a UUID suffix is appended to the
/// filename to guarantee uniqueness.  When `on_conflict` is `Some`, the
/// original filename is used and the conflict strategy is applied.
///
/// Returns the absolute path of the saved file, or `None` if `on_conflict` is
/// `Skip` and the destination already exists.
async fn save_file(
    file: &mut TempFile<'_>,
    target_dir: &Path,
    filename: String,
    extension: String,
    last_modified_ms: u64,
    on_conflict: Option<OnConflict>,
) -> Result<Option<String>, AppError> {
    let unique_id = Uuid::new_v4();
    let target_dir = target_dir.to_path_buf();

    let tmp_path = target_dir.join(format!("{filename}-{unique_id}.tmp"));

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
        let base_final = if on_conflict.is_some() {
            target_dir.join(format!("{filename_owned}.{extension}"))
        } else {
            target_dir.join(format!("{filename_owned}-{unique_id}.{extension}"))
        };

        let final_path = if let Some(strategy) = on_conflict {
            if base_final.exists() {
                match strategy {
                    OnConflict::Skip => {
                        // Remove the temp file and signal nothing to index.
                        let _ = std::fs::remove_file(&tmp_path_owned);
                        return Ok(None);
                    }
                    OnConflict::Replace => base_final,
                    OnConflict::Rename => find_unique_upload_path(&base_final)?,
                }
            } else {
                base_final
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

/// Append `-NNN` before the extension until we find a path that doesn't exist.
/// `photo.jpg` → `photo-001.jpg`, `photo-002.jpg`, … Gives up after
/// `u32::MAX - 1` collisions and returns an error instead of panicking; a
/// filesystem cannot realistically hold that many variants.
fn find_unique_upload_path(base: &Path) -> Result<PathBuf, AppError> {
    let stem = base.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = base.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parent = base.parent().unwrap_or(Path::new("."));

    for n in 1u32..u32::MAX {
        let name = if ext.is_empty() {
            format!("{stem}-{n:03}")
        } else {
            format!("{stem}-{n:03}.{ext}")
        };
        let candidate = parent.join(&name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(AppError::new(
        ErrorKind::IO,
        format!("Could not find a free filename for {}", base.display()),
    ))
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

/// Reject uploads whose bytes do not match the `Content-Type`-derived
/// extension. Enabled via `validate_upload_content`. Detection is signature
/// based via the `infer` crate — never a full decode — so unusual-but-valid
/// variants still pass; the stored extension itself is still taken from the
/// declared `Content-Type`.
fn validate_upload_content(file: &TempFile<'_>, extension: &str) -> Result<(), AppError> {
    use std::io::Read;
    let path = file
        .path()
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Uploaded file is empty"))?;
    let mut head = [0u8; 512];
    let mut reader = std::fs::File::open(path)
        .or_raise(|| (ErrorKind::IO, "Failed to read uploaded file for validation"))?;
    let n = reader
        .read(&mut head)
        .or_raise(|| (ErrorKind::IO, "Failed to read uploaded file for validation"))?;
    let head = &head[..n];

    let Some(detected) = infer::get(head) else {
        return Err(unrecognized_upload_content(extension));
    };
    let detected_ext = detected.extension();
    let matches = match extension {
        // JPEG family: jpg/jpeg/jfif/jpe are byte-identical in signature.
        "jpg" | "jpeg" | "jfif" | "jpe" => detected_ext == "jpg",
        "tif" | "tiff" => detected_ext == "tif",
        // MP4 and QuickTime share the ISO BMFF box structure.
        "mp4" | "mov" | "m4v" => matches!(detected_ext, "mp4" | "mov" | "m4v"),
        // Matroska and WebM share the EBML container.
        "mkv" | "webm" => matches!(detected_ext, "mkv" | "webm"),
        // The whitelist spells the MPEG-PS extension "mpeg"; infer uses "mpg".
        "mpeg" => detected_ext == "mpg",
        // Remaining whitelisted types (png, webp, bmp, gif, avi, flv, wmv)
        // map 1:1 onto infer's canonical extension.
        other => detected_ext == other,
    };
    if matches {
        Ok(())
    } else {
        Err(mismatched_upload_content(extension, detected_ext))
    }
}

fn unrecognized_upload_content(extension: &str) -> AppError {
    AppError::new(
        ErrorKind::InvalidInput,
        format!("Uploaded content is not recognized as {extension}"),
    )
}

fn mismatched_upload_content(extension: &str, detected: &str) -> AppError {
    AppError::new(
        ErrorKind::InvalidInput,
        format!("Uploaded content is {detected}, but the declared type is {extension}"),
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
