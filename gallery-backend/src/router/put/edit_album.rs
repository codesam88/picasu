use crate::public::constant::redb::DATA_TABLE;
use crate::public::db::tree::TREE;
use crate::public::error::{AppError, ErrorKind, ResultExt};
use crate::public::structure::abstract_data::AbstractData;
use crate::router::fairing::guard_auth::GuardAuth;
use crate::router::fairing::guard_read_only_mode::GuardReadOnlyMode;
use crate::router::fairing::guard_share::GuardShare;
use crate::router::{AppResult, GuardResult};
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use anyhow::Result;
use arrayvec::ArrayString;
use redb::ReadableTable;
use rocket::serde::{Deserialize, json::Json};
use serde::Serialize;

/// Payload for updating a specific album's cover image.
#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetAlbumCover {
    pub album_id: ArrayString<64>,
    /// The hash of the image to set as cover.
    pub cover_hash: ArrayString<64>,
}

/// Updates the cover image of a specific album.
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
pub struct SetAlbumTitle {
    pub album_id: ArrayString<64>,
    pub title: Option<String>,
}

/// Updates the display title of a specific album.
#[put("/put/set_album_title", data = "<set_album_title>")]
pub async fn set_album_title(
    auth: GuardResult<GuardShare>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    set_album_title: Json<SetAlbumTitle>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;

    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let set_album_title_inner = set_album_title.into_inner();
        let album_id = set_album_title_inner.album_id;

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

            album.metadata.title = set_album_title_inner.title;
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
