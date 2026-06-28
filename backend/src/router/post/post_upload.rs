use crate::constant::{VALID_IMAGE_EXTENSIONS, VALID_VIDEO_EXTENSIONS};
use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::config::APP_CONFIG;
use crate::process::dir_album::get_dir_path_for_album;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::auth::GuardUpload;
use crate::router::{AppResult, GuardResult};
use crate::storage::files::get_resolved_image_home;
use anyhow::Result;
use arrayvec::ArrayString;
use rocket::form::{Errors, Form};
use rocket::fs::TempFile;
use std::path::{Path, PathBuf};
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
    file.name()
        .map(std::string::ToString::to_string)
        .unwrap_or_default()
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
#[post("/upload?<presigned_album_id_opt>", data = "<form>")]
pub async fn upload(
    auth: GuardResult<GuardUpload>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    presigned_album_id_opt: Option<String>,
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

    for (i, file) in inner_form.files.iter_mut().enumerate() {
        let last_modified = inner_form.last_modified[i];
        let filename = get_filename(file);
        let extension = get_extension(file)?;

        if VALID_IMAGE_EXTENSIONS.contains(&extension.as_str())
            || VALID_VIDEO_EXTENSIONS.contains(&extension.as_str())
        {
            let final_path =
                save_file(file, &target_dir, filename, extension, last_modified).await?;
            let image_root = get_resolved_image_home()
                .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "No imagePath configured"))?;
            let relative_src = Path::new(&final_path)
                .strip_prefix(&image_root)
                .map_err(|_| {
                    AppError::new(ErrorKind::Internal, "Uploaded file path outside IMAGE_HOME")
                })?;
            crate::workflow::index_image(relative_src, None)
                .await
                .or_raise(|| (ErrorKind::Internal, "Failed to index file"))?;
        } else {
            error!("Rejected invalid file type: {}", extension);
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                format!("Invalid file type: {extension}"),
            ));
        }
    }

    Ok(())
}

/// Persists the temporary file directly into `target_dir` (its real, final
/// location under `IMAGE_HOME`) with the correct modification time.
///
/// Returns the absolute path of the saved file.
async fn save_file(
    file: &mut TempFile<'_>,
    target_dir: &Path,
    filename: String,
    extension: String,
    last_modified_ms: u64,
) -> Result<String, AppError> {
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
    // This ensures the file watcher (workflow) only picks up the file once it is fully written and has the correct timestamp.
    let final_path = spawn_blocking(move || -> Result<String, AppError> {
        let final_path = target_dir.join(format!("{filename_owned}-{unique_id}.{extension}"));

        set_last_modified_time(&tmp_path_owned, last_modified_ms)?;
        std::fs::rename(&tmp_path_owned, &final_path)
            .or_raise(|| (ErrorKind::IO, "Failed to rename file"))?;

        Ok(final_path.to_string_lossy().into_owned())
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    Ok(final_path)
}

#[allow(clippy::cast_possible_wrap)]
fn set_last_modified_time(path: &Path, last_modified_ms: u64) -> Result<(), AppError> {
    let mtime = filetime::FileTime::from_unix_time((last_modified_ms / 1000) as i64, 0);
    filetime::set_file_mtime(path, mtime)
        .or_raise(|| (ErrorKind::IO, "Failed to set file modification time"))?;
    Ok(())
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
