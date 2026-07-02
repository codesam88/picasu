use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::model::album::AlbumCombined;
use crate::process::xmp_write::write_sidecar_for;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::auth::GuardShare;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::DATA_TABLE;
use crate::storage::db::TREE;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use anyhow::Result;
use arrayvec::ArrayString;
use log::warn;
use redb::ReadableTable;
use rocket::serde::{Deserialize, json::Json};
use serde::Serialize;

/// Read-modify-write a single album record: fetch it, apply `mutate`, write
/// its `.albuminfo.xmp` sidecar (best-effort — logged, not fatal), and commit.
/// Shared by every single-field album edit endpoint below.
fn update_album(
    album_id: ArrayString<64>,
    mutate: impl FnOnce(&mut AlbumCombined),
) -> Result<(), AppError> {
    let txn = TREE
        .in_disk
        .begin_write()
        .or_raise(|| (ErrorKind::Database, "Failed to begin transaction"))?;
    {
        let mut data_table = txn
            .open_table(DATA_TABLE)
            .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

        let album = data_table
            .get(&*album_id)
            .or_raise(|| (ErrorKind::Database, "Failed to get album"))?
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Album not found"))?
            .value();
        let AbstractData::Album(mut album) = album else {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "Expected Album but got different type",
            ));
        };

        mutate(&mut album);

        let abstract_data = AbstractData::Album(album);
        if let Err(e) = write_sidecar_for(&abstract_data) {
            warn!("Failed to write XMP sidecar: {e}");
        }
        data_table
            .insert(&*album_id, abstract_data)
            .or_raise(|| (ErrorKind::Database, "Failed to update album"))?;
    }
    txn.commit()
        .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
    Ok(())
}

/// Payload for updating a specific album's cover image.
#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct SetAlbumCover {
    #[schema(value_type = String)]
    pub album_id: ArrayString<64>,
    /// The hash of the image to set as cover.
    #[schema(value_type = String)]
    pub cover_hash: ArrayString<64>,
}

/// Updates the cover image of a specific album.
#[utoipa::path(
        put,
        path = "/put/set_album_cover",
        request_body = SetAlbumCover,
        responses(
            (status = 200, description = "Album cover updated"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[put("/put/set_album_cover", data = "<set_album_cover>")]
pub async fn set_album_cover(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    set_album_cover: Json<SetAlbumCover>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;

    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let set_album_cover_inner = set_album_cover.into_inner();
        let album_id = set_album_cover_inner.album_id;
        let cover_hash = set_album_cover_inner.cover_hash;

        let txn = TREE
            .in_disk
            .begin_write()
            .or_raise(|| (ErrorKind::Database, "Failed to begin transaction"))?;
        {
            let mut data_table = txn
                .open_table(DATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

            let album = data_table
                .get(&*album_id)
                .or_raise(|| (ErrorKind::Database, "Failed to get album"))?
                .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Album not found"))?
                .value();
            let AbstractData::Album(mut album) = album else {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    "Expected Album but got different type",
                ));
            };
            let database = data_table
                .get(&*cover_hash)
                .or_raise(|| (ErrorKind::Database, "Failed to get cover image"))?
                .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Cover image not found"))?
                .value();

            album.set_cover(&database);
            data_table
                .insert(&*album_id, AbstractData::Album(album))
                .or_raise(|| (ErrorKind::Database, "Failed to update album"))?;
        }
        txn.commit()
            .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
        Ok(())
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;
    Ok(())
}

/// Payload for renaming an album.
#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct SetAlbumTitle {
    #[schema(value_type = String)]
    pub album_id: ArrayString<64>,
    pub title: Option<String>,
}

/// Updates the display title of a specific album.
///
/// Sets both the raw display `title` and `custom_title` (the value actually
/// persisted to `.albuminfo.xmp` by `write_sidecar_for`). Clearing the title
/// (`title: None`) falls back `title` to the directory-derived default for
/// dir-albums, so the sidecar-freezing bug can't reappear via this path.
#[utoipa::path(
        put,
        path = "/put/set_album_title",
        request_body = SetAlbumTitle,
        responses(
            (status = 200, description = "Album title updated"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[put("/put/set_album_title", data = "<set_album_title>")]
pub async fn set_album_title(
    auth: GuardResult<GuardShare>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    set_album_title: Json<SetAlbumTitle>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;

    let set_album_title_inner = set_album_title.into_inner();
    let album_id = set_album_title_inner.album_id;

    tokio::task::spawn_blocking(move || {
        update_album(album_id, |album| {
            album
                .metadata
                .custom_title
                .clone_from(&set_album_title_inner.title);
            album.metadata.title = set_album_title_inner.title.or_else(|| {
                Some(crate::process::dir_album::derive_default_title(
                    &album.metadata.dir_path,
                ))
            });
        })
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;

    Ok(())
}
