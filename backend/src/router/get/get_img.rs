use crate::error::{AppError, ErrorKind, ResultExt};
use crate::openapi_components::Unauthorized;
use crate::router::{
    AppResult, GuardResult,
    auth::{GuardHash, GuardHashOriginal, GuardShare},
};
use crate::storage::files::get_data_path;
use rocket::fs::NamedFile;
use rocket::http::ContentType;
use rocket::request::Request;
use rocket::response::{Responder, Result as ResponseResult};
use rocket_seek_stream::SeekStream;
use std::path::PathBuf;

pub enum CompressedFileResponse<'a> {
    SeekStream(SeekStream<'a>),
    NamedFile(NamedFile),
}

impl<'r> Responder<'r, 'static> for CompressedFileResponse<'static> {
    fn respond_to(self, request: &'r Request<'_>) -> ResponseResult<'static> {
        match self {
            CompressedFileResponse::SeekStream(stream) => {
                // Compressed video is always mp4, so pin the Content-Type to
                // the stored extension instead of letting SeekStream sniff the
                // bytes (which would leak the spoofed/mislabeled type to
                // clients). Range requests for video seeking are unaffected.
                let mut response = stream.respond_to(request)?;
                response.set_header(ContentType::new("video", "mp4"));
                Ok(response)
            }
            CompressedFileResponse::NamedFile(named_file) => named_file.respond_to(request),
        }
    }
}

/// Serve the compressed thumbnail or preview of a hashed asset.
///
/// Resolves the requested path under `DATA_HOME/object/compressed` and serves
/// `.jpg` in a single response or `.mp4` as a range-capable stream pinned to
/// `video/mp4`.
///
/// Corner cases: one image-serving token authorizes exactly one file — its
/// `hash` claim must equal the id in the last path segment. Any extension other
/// than `.jpg` or `.mp4`, and a path without an extension, are rejected as
/// invalid input.
///
/// Errors: 400 unsupported or missing file extension — 401 no valid admin or
/// share credentials, or no image-serving token for this file — 500 the
/// compressed file could not be opened.
#[utoipa::path(
        tag = "serving",
        responses(
            (status = 200, description = "Compressed file"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/object/compressed/<file_path..>")]
pub async fn compressed_file(
    auth_guard: GuardResult<GuardShare>,
    hash_guard: GuardResult<GuardHash>,
    file_path: PathBuf,
) -> AppResult<CompressedFileResponse<'static>> {
    let _ = auth_guard?;
    let _ = hash_guard?;
    let root = get_data_path();
    let compressed_file_path = root.join("object/compressed").join(&file_path);

    let result = match compressed_file_path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
    {
        Some("mp4") => SeekStream::from_path(&compressed_file_path)
            .map(CompressedFileResponse::SeekStream)
            .or_raise(|| {
                (
                    ErrorKind::IO,
                    format!(
                        "Failed to open MP4 file: {}",
                        compressed_file_path.display()
                    ),
                )
            })?,
        Some("jpg") => {
            let named_file = NamedFile::open(&compressed_file_path).await.or_raise(|| {
                (
                    ErrorKind::IO,
                    format!(
                        "Failed to open JPG file: {}",
                        compressed_file_path.display()
                    ),
                )
            })?;
            CompressedFileResponse::NamedFile(named_file)
        }
        Some(ext) => {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                format!("Unsupported file extension: {ext}"),
            )
            .context(format!("File path: {}", compressed_file_path.display())));
        }
        None => {
            return Err(
                AppError::new(ErrorKind::InvalidInput, "File has no extension")
                    .context(format!("File path: {}", compressed_file_path.display())),
            );
        }
    };

    Ok(result)
}

/// Serve the original file from its current location under `imagePath`.
///
/// There is no copy under `DATA_HOME`: `imagePath` holds the single
/// authoritative copy. The last path segment is `<id>.<ext>`, where `id` is the
/// asset ID, and the record for that ID names the location the file is streamed
/// from.
///
/// Corner cases: resolving is by asset ID alone — there is no hash fallback —
/// and the image-serving token must both grant original access and name this
/// asset in its `asset_id` claim.
///
/// Errors: 400 the path carries no usable asset id — 401 no valid admin or
/// share credentials, or no image-serving token granting original access —
/// 404 unknown asset id — 500 the asset record or the file could not be read.
#[utoipa::path(
        tag = "serving",
        responses(
            (status = 200, description = "Imported original file"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
            (status = 404, description = "Not found"),
        )
    )
]
#[get("/object/imported/<file_path..>")]
pub async fn imported_file(
    auth: GuardResult<GuardShare>,
    hash_guard: GuardResult<GuardHashOriginal>,
    file_path: PathBuf,
) -> AppResult<CompressedFileResponse<'static>> {
    let _ = auth?;
    let _ = hash_guard?;

    let id_str = file_path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Invalid file path: missing id"))?
        .to_string();

    let source_path = tokio::task::spawn_blocking(move || -> AppResult<PathBuf> {
        // Resolve by asset_id only. No hash fallback — asset ID is
        // authoritative for original serving.
        let asset_id: arrayvec::ArrayString<64> = id_str
            .parse()
            .map_err(|_| AppError::new(ErrorKind::InvalidInput, "Invalid asset_id format"))?;
        let record = crate::storage::asset_store::get_asset_by_id(&asset_id)
            .or_raise(|| (ErrorKind::Database, "Failed to fetch asset record"))?
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Asset not found"))?;
        Ok(std::path::PathBuf::from(&record.path))
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    NamedFile::open(&source_path)
        .await
        .map(CompressedFileResponse::NamedFile)
        .or_raise(|| {
            (
                ErrorKind::IO,
                format!("Error opening original file: {}", source_path.display()),
            )
        })
}
