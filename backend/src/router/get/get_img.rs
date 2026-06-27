use crate::error::{AppError, ErrorKind, ResultExt};
use crate::router::{
    AppResult, GuardResult,
    auth::{GuardHash, GuardHashOriginal, GuardShare},
};
use crate::storage::db::open_data_table;
use crate::storage::files::get_data_path;
use rocket::fs::NamedFile;
use rocket::response::Responder;
use rocket_seek_stream::SeekStream;
use std::path::PathBuf;

#[derive(Responder)]
pub enum CompressedFileResponse<'a> {
    SeekStream(SeekStream<'a>),
    NamedFile(NamedFile),
}

#[utoipa::path(
        get,
        path = "/object/compressed/{file_path}",
        responses(
            (status = 200, description = "Compressed file"),
            (status = 400, description = "Invalid input"),
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

/// Serve the original file directly from its current location under
/// `imagePath` — there is no copy of it under `DATA_HOME`; `IMAGE_HOME` is
/// the single, authoritative copy (see `docs/design.md` "Albums" and
/// `TODO.md`'s "Storage architecture fix"). The route's `<file_path..>`
/// segment is still `<hash-prefix>/<hash>.<ext>` for URL compatibility with
/// the frontend and `GuardHashOriginal`'s validation, but only the hash
/// (the file stem) is actually used, to look up the record's current
/// `source_path()`.
#[utoipa::path(
        get,
        path = "/object/imported/{file_path}",
        responses(
            (status = 200, description = "Imported original file"),
            (status = 400, description = "Invalid input"),
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

    let hash = file_path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Invalid file path: missing hash"))?
        .to_string();

    let source_path = tokio::task::spawn_blocking(move || -> AppResult<PathBuf> {
        let data_table = open_data_table();
        let abstract_data = data_table
            .get(hash.as_str())
            .or_raise(|| (ErrorKind::Database, "Failed to fetch DB record"))?
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Hash not found"))?
            .value();
        Ok(abstract_data.source_path())
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
