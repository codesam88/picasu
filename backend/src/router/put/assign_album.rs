use crate::error::{AppError, ErrorKind, ResultExt};

use crate::process::dir_album::{
    get_dir_path_for_album, get_parent_album_id, mark_album_for_update,
    rewrite_dir_album_cache_prefix,
};
use crate::process::sanitize::find_unique_path;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::VERSION_COUNT_TIMESTAMP;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::INDEX_COORDINATOR;
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use arrayvec::ArrayString;
use rocket::serde::{Deserialize, Serialize, json::Json};
use std::fs;
use std::path::{Path, PathBuf};

/// Filename-collision strategy for moves and uploads: `skip` leaves an
/// existing destination untouched (the source stays put, outcome `skipped`);
/// `rename` lands the file under a unique suffixed name (outcome
/// `renamedFrom`). Required on assign with no default; upload defaults to
/// `rename`.
#[derive(Debug, Deserialize, utoipa::ToSchema, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum OnConflict {
    Skip,
    Rename,
}

/// Request body for `PUT /put/assign_album`. Strict: unknown fields (including
/// the legacy multi-alias `alias` path) are rejected rather than ignored.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignAlbumData {
    /// Path-primary asset ID. The handler resolves the record and its
    /// physical path via `ASSET_BY_ID`, allowing independent
    /// movement of same-hash files.
    #[schema(value_type = String)]
    pub asset_id: ArrayString<64>,
    /// Destination album ID; must be a filesystem-backed directory album
    /// (manual albums are rejected with 400).
    #[schema(value_type = String)]
    pub album_id: ArrayString<64>,
    pub on_conflict: OnConflict,
}

/// Outcome of an `assign_album` call, reported to the caller so the UI is never
/// silent about what happened to the selected item.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignResult {
    pub outcome: AssignOutcome,
}

/// The concrete result of a successful assign: `moved`, `renamedFrom` (an
/// auto-`-001` suffix collision), or `skipped` (destination already exists and
/// strategy is skip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum AssignOutcome {
    Moved,
    RenamedFrom,
    Skipped,
}

/// Move the asset identified by `asset_id` into the album's directory on disk
/// (resolved from its physical path), update the stored path and
/// album membership, and report the conflict outcome. Returns 400 if the file
/// is missing at the asset's path (stale record — re-index first).
#[utoipa::path(
        put,
        path = "/put/assign_album",
        tag = "albums",
        summary = "Move an asset into an album",
        description = "Moves the file identified by asset_id into the album directory on disk, updates stored path and album membership, and reports the conflict outcome. Returns 400 when the file is missing at the asset's path (stale record) or the destination is a manual album.",
        request_body = AssignAlbumData,
        responses(
            (status = 200, description = "Item assigned to album", body = AssignResult),
            (status = 400, description = "Invalid input or item not found"),
        )
    )
]
#[put("/put/assign_album", format = "json", data = "<json_data>")]
pub async fn assign_album(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<AssignAlbumData>,
) -> AppResult<Json<AssignResult>> {
    let _ = auth?;
    let _ = read_only_mode?;

    let data = json_data.into_inner();
    let asset_id = data.asset_id;
    let album_id = data.album_id;
    let on_conflict = data.on_conflict;

    // Resolve album's directory from the in-memory cache.
    let album_dir = get_dir_path_for_album(album_id)
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album not found in dir cache"))?;

    if !album_dir.is_dir() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!(
                "Album directory no longer exists on disk: {} — re-index to refresh",
                album_dir.display()
            ),
        ));
    }

    let outcome = tokio::task::spawn_blocking(move || {
        move_asset_into_album(asset_id, album_id, &album_dir, on_conflict)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;

    // Bump the version counter so subsequent prefetch calls create a new
    // query cache entry instead of returning stale data from the snapshot
    // taken before the mutation.  (UpdateExpireTask, which normally
    // advances this counter, runs asynchronously — too late for the next
    // frontend request.)
    VERSION_COUNT_TIMESTAMP.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    INDEX_COORDINATOR
        .execute_waiting(AlbumSelfUpdateTask::new(album_id))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update album stats"))?
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("Album update failed: {e}")))?;

    Ok(Json(AssignResult { outcome }))
}

/// Move a single asset (identified by `asset_id`) into `album_dir`.
/// This is the path-primary move: only the one physical file at the asset's
/// path is moved, regardless of hash-matched duplicates.
/// Albums move as directory trees via `move_album_into_album`.
fn move_asset_into_album(
    asset_id: ArrayString<64>,
    album_id: ArrayString<64>,
    album_dir: &Path,
    on_conflict: OnConflict,
) -> Result<AssignOutcome, AppError> {
    use crate::storage::asset_store;

    // Look up the asset record.
    let record = asset_store::get_asset_by_id(&asset_id)
        .or_raise(|| (ErrorKind::Database, "Failed to look up asset"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Asset not found"))?;

    // Albums move as directory trees.
    if record.kind == crate::model::asset::AssetKind::Album {
        return move_album_into_album(asset_id, album_id, album_dir, on_conflict);
    }

    let source_path = PathBuf::from(&record.path);
    if !source_path.exists() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!("File not found at: {}", source_path.display()),
        ));
    }

    let file_name = source_path
        .file_name()
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "File has no name"))?;
    let base_dest = album_dir.join(file_name);

    let (final_dest, outcome) = if base_dest.exists() {
        if source_path == base_dest {
            return Ok(AssignOutcome::Moved); // already there
        }
        match on_conflict {
            OnConflict::Skip => return Ok(AssignOutcome::Skipped),
            OnConflict::Rename => {
                let unique = crate::process::sanitize::find_unique_path(&base_dest)
                    .or_raise(|| (ErrorKind::IO, "Failed to find unique path"))?;
                (unique, AssignOutcome::RenamedFrom)
            }
        }
    } else {
        (base_dest, AssignOutcome::Moved)
    };

    // Move the file on disk.
    fs::rename(&source_path, &final_dest).or_raise(|| (ErrorKind::IO, "Failed to move file"))?;

    // Move the sidecar if it exists.
    let sidecar = source_path.with_extension("xmp");
    if sidecar.exists() {
        let new_sidecar = final_dest.with_extension("xmp");
        let _ = fs::rename(&sidecar, &new_sidecar);
    }

    // Update the asset record with the new path and album.
    let new_path = final_dest.to_string_lossy().into_owned();
    let mut updated = record.clone();
    updated.path.clone_from(&new_path);
    updated.album_id = Some(album_id);

    // Update ASSET_BY_ID.
    asset_store::put_asset_by_id(&updated)
        .or_raise(|| (ErrorKind::Database, "Failed to update asset"))?;

    // Update ASSET_BY_PATH: remove old, add new.
    asset_store::remove_asset_by_path(&record.path)
        .or_raise(|| (ErrorKind::Database, "Failed to remove old path mapping"))?;
    asset_store::put_asset_by_path(&new_path, asset_id)
        .or_raise(|| (ErrorKind::Database, "Failed to add new path mapping"))?;

    // The metadata payload holds no path or album fields — composition
    // derives both from the updated AssetRecord — so no METADATA_TABLE
    // write is needed here.

    Ok(outcome)
}

/// Move a sub-album's whole directory into `target_dir` (another album's
/// directory), then update every DB record whose path lived under the old
/// directory.
///
/// When the descendant directory name does not collide with an existing target
/// path, the whole tree is renamed to `target_dir/<name>/...` and every record
/// under the old prefix is rewritten. The physical `fs::rename` carries each
/// file's `.xmp` sidecar along, so only the stored path *strings* need
/// updating.
///
/// On a name collision (`base_dest` already exists):
/// - `Skip` leaves both source and target untouched, reported as `Skipped`.
/// - `Rename` renames the whole directory to a unique `-001` sibling
///   (`find_unique_path`) with the same path-rewrite, reported as
///   `RenamedFrom`.
fn move_album_into_album(
    album_id: ArrayString<64>,
    target_album_id: ArrayString<64>,
    target_dir: &Path,
    on_conflict: OnConflict,
) -> Result<AssignOutcome, AppError> {
    // The album directory path is identity — read it from AssetRecord.
    let record = crate::storage::asset_store::get_asset_by_id(&album_id)
        .or_raise(|| (ErrorKind::Database, "Failed to look up album"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album not found"))?;
    let source_dir = PathBuf::from(&record.path);
    if !source_dir.is_dir() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!(
                "Album directory no longer exists on disk: {} — re-index to refresh",
                source_dir.display()
            ),
        ));
    }

    if target_dir == source_dir || target_dir.starts_with(&source_dir) {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "Cannot move an album into itself or one of its own sub-albums",
        ));
    }

    let dir_name = source_dir
        .file_name()
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album directory has no name"))?;
    let base_dest = target_dir.join(dir_name);

    let (new_dir, outcome) = if base_dest.exists() {
        match on_conflict {
            OnConflict::Skip => return Ok(AssignOutcome::Skipped),
            OnConflict::Rename => {
                let dest_dir = find_unique_path(&base_dest)?;
                rename_dir(&source_dir, &dest_dir)?;
                (Some(dest_dir), AssignOutcome::RenamedFrom)
            }
        }
    } else {
        // No collision: land the whole tree in place (Moved).
        let dest_dir = base_dest;
        rename_dir(&source_dir, &dest_dir)?;
        (Some(dest_dir), AssignOutcome::Moved)
    };

    if let Some(new_dir) = new_dir {
        rewrite_dir_album_cache_prefix(&source_dir, &new_dir);
        // Update asset tables for all moved files. Path rewrites live
        // exclusively on the AssetRecords now — the metadata payload holds
        // no paths.
        let _ = update_asset_tables_after_dir_move(&source_dir, &new_dir);
    }

    if let Some(old_parent_id) = get_parent_album_id(&source_dir) {
        mark_album_for_update(old_parent_id);
    }
    mark_album_for_update(target_album_id);

    Ok(outcome)
}

/// `fs::rename` a whole album directory from `source_dir` to `dest_dir`.
fn rename_dir(source_dir: &Path, dest_dir: &Path) -> Result<(), AppError> {
    fs::rename(source_dir, dest_dir).map_err(|e| {
        AppError::new(
            ErrorKind::Internal,
            format!("Failed to move album directory: {e}"),
        )
    })
}

/// Update asset tables after a directory move.
/// For each asset whose path starts with `source_dir`, update it
/// to the corresponding path under `dest_dir`.
fn update_asset_tables_after_dir_move(source_dir: &Path, dest_dir: &Path) -> Result<(), AppError> {
    use crate::storage::asset_store;

    let records = asset_store::get_all_assets()
        .or_raise(|| (ErrorKind::Database, "Failed to read asset records"))?;

    for record in records {
        let old_path = PathBuf::from(&record.path);
        if let Ok(rel) = old_path.strip_prefix(source_dir) {
            let new_path = dest_dir.join(rel);
            let new_path_str = new_path.to_string_lossy().into_owned();

            // Update ASSET_BY_PATH: remove old, add new.
            let _ = asset_store::remove_asset_by_path(&record.path);
            let _ = asset_store::put_asset_by_path(&new_path_str, record.asset_id);

            // Update path in ASSET_BY_ID.
            let mut updated = record.clone();
            updated.path = new_path_str;
            let _ = asset_store::put_asset_by_id(&updated);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Path-primary request contract: `assetId` identifies the one physical
    /// file (resolved server-side via its path). There is no
    /// caller-supplied `alias` path in the body, and `onConflict` remains
    /// required with no default.
    #[test]
    fn assign_album_data_schema_is_path_primary() {
        let spec: serde_json::Value = serde_json::from_str(&crate::openapi::generate_json())
            .expect("generated OpenAPI must be valid JSON");
        let schema = &spec["components"]["schemas"]["AssignAlbumData"];
        let properties = schema["properties"]
            .as_object()
            .expect("AssignAlbumData must declare properties");

        assert!(
            !properties.contains_key("alias"),
            "alias must not appear in AssignAlbumData; asset_id resolves the path"
        );

        let mut required: Vec<&str> = schema["required"]
            .as_array()
            .expect("AssignAlbumData must declare required fields")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        required.sort_unstable();
        assert_eq!(required, ["albumId", "assetId", "onConflict"]);
    }

    /// The path-primary body (assetId + albumId + onConflict, no alias)
    /// deserializes into the handler's request type.
    #[test]
    fn assign_album_data_deserializes_without_alias() {
        let data: AssignAlbumData = serde_json::from_str(
            r#"{"assetId":"asset-1","albumId":"album-1","onConflict":"rename"}"#,
        )
        .expect("path-primary body without alias must deserialize");
        assert_eq!(&*data.asset_id, "asset-1");
        assert_eq!(&*data.album_id, "album-1");
        assert_eq!(data.on_conflict, OnConflict::Rename);
    }

    /// A legacy body that still carries `alias` is rejected: the field is
    /// not part of the path-primary contract, and backward compatibility
    /// with the multi-alias request shape is not a goal.
    #[test]
    fn assign_album_data_rejects_legacy_alias_field() {
        let err = serde_json::from_str::<AssignAlbumData>(
            r#"{"assetId":"asset-1","albumId":"album-1","onConflict":"rename","alias":"/old/path.jpg"}"#,
        )
        .expect_err("legacy alias field must be rejected");
        assert!(
            err.to_string().contains("alias"),
            "rejection must name the unknown field: {err}"
        );
    }

    /// Public-contract polish: a single-line summary (multi-line summaries
    /// become invalid HTML anchors in the generated reference), a real tag,
    /// and docs on the conflict enum and the destination field.
    #[test]
    fn assign_album_openapi_summary_tag_and_field_docs() {
        let spec: serde_json::Value = serde_json::from_str(&crate::openapi::generate_json())
            .expect("generated OpenAPI must be valid JSON");

        let op = &spec["paths"]["/put/assign_album"]["put"];
        let tags = op["tags"].as_array().expect("assign must declare tags");
        assert!(
            tags.iter().any(|t| t == "albums"),
            "assign_album must be tagged `albums`: {tags:?}"
        );
        let summary = op["summary"].as_str().expect("assign must have a summary");
        assert!(
            !summary.contains('\n'),
            "summary must be a single line (raw newlines break reference anchors): {summary:?}"
        );

        let on_conflict_desc = spec["components"]["schemas"]["OnConflict"]["description"]
            .as_str()
            .unwrap_or_default();
        assert!(
            !on_conflict_desc.is_empty(),
            "OnConflict must describe the skip/rename semantics"
        );

        let album_id_desc = spec["components"]["schemas"]["AssignAlbumData"]["properties"]
            ["albumId"]["description"]
            .as_str()
            .unwrap_or_default();
        assert!(
            !album_id_desc.is_empty(),
            "AssignAlbumData.albumId must be documented"
        );
    }
}
