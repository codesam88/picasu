use crate::error::{AppError, ErrorKind, ResultExt};
use crate::openapi_components::Unauthorized;

use crate::model::asset::AssetRecord;
use crate::process::dir_album::{get_dir_path_for_album, rewrite_dir_album_cache_prefix};
use crate::process::sanitize::find_unique_path;
use crate::router::auth::GuardMutate;
use crate::router::auth::GuardUser;
use crate::router::{AppResult, GuardResult};

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

/// Move one asset, or one album directory, into another album.
///
/// `assetId` addresses the asset record and `albumId` the destination album,
/// whose directory is resolved from the album directory cache. Only the file at
/// the asset's own path moves, never hash-matched duplicates: the stored path
/// and album membership are rewritten to match, an album moves as its whole
/// directory tree with every descendant record's path rewritten, and a `.xmp`
/// sidecar rides along with the file. Album aggregates are maintained by deltas;
/// the query-cache version is bumped before the response returns, so the next
/// prefetch already reflects the move.
///
/// Corner cases: `onConflict` is required and has no default — `skip` leaves an
/// existing destination untouched and reports `skipped`, `rename` lands under a
/// suffixed name and reports `renamedFrom`, a plain move reports `moved`. An
/// asset already sitting in the destination directory reports `moved` without
/// anything being rewritten, and an album moved into itself or into one of its
/// own sub-albums is rejected. Compressed thumbnails and videos are keyed by
/// content hash, not by path, so no derivative moves or is invalidated.
///
/// Errors: 400 unknown asset or album id, a source path or album directory that
/// no longer exists, or a destination album that has no directory — 401 missing
/// or invalid credentials; share tokens are not accepted — 405 read-only
/// mode — 500 storage failure.
#[quint::event("API_ASSIGN_ALBUM_DIR_MOVE")]
#[quint::operation("OP-4")]
#[utoipa::path(
        tag = "albums",
        request_body = AssignAlbumData,
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
            (status = 200, description = "Item assigned to album", body = AssignResult),
            (status = 400, description = "Invalid input or item not found"),
            (status = 404, description = "Not found"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/assign_album", format = "json", data = "<json_data>")]
pub async fn assign_album(
    auth: GuardResult<GuardUser>,
    mutation: GuardResult<GuardMutate>,
    json_data: Json<AssignAlbumData>,
) -> AppResult<Json<AssignResult>> {
    let GuardUser { claims } = auth?;
    let _ = mutation?;
    let (caller, _) = crate::router::put::users_password::caller_identity(&claims);

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

    // Write gate: destination album must be writable by the caller.
    crate::storage::files::write_gate(&caller, None, Some(&album_dir)).map_err(|_| {
        AppError::new(
            ErrorKind::PermissionDenied,
            "Destination album not writable",
        )
    })?;

    let outcome = tokio::task::spawn_blocking(move || {
        move_asset_into_album(asset_id, album_id, &album_dir, on_conflict, &caller)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    crate::storage::tree_view::tree_changed();

    Ok(Json(AssignResult { outcome }))
}

/// Move a single asset (identified by `asset_id`) into `album_dir`.
/// This is the path-primary move: only the one physical file at the asset's
/// path is moved, regardless of hash-matched duplicates.
/// Albums move as directory trees via `move_album_into_album`.
#[quint::operation("OP-4")]
#[allow(warnings)]
fn move_asset_into_album(
    asset_id: ArrayString<64>,
    album_id: ArrayString<64>,
    album_dir: &Path,
    on_conflict: OnConflict,
    caller: &str,
) -> Result<AssignOutcome, AppError> {
    use crate::storage::asset_store;

    // Look up the asset record.
    let record = asset_store::get_asset_by_id(&asset_id)
        .or_raise(|| (ErrorKind::Database, "Failed to look up asset"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Asset not found"))?;

    // Albums move as directory trees.
    if record.kind == crate::model::asset::AssetKind::Album {
        return move_album_into_album(asset_id, album_id, album_dir, on_conflict, caller);
    }

    let source_path = PathBuf::from(&record.path);
    if !source_path.exists() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!("File not found at: {}", source_path.display()),
        ));
    }

    // Write gate: source must be writable by caller and not cross-owner vs dest.
    crate::storage::files::write_gate(caller, Some(&source_path), Some(album_dir)).map_err(
        |_| {
            AppError::new(
                ErrorKind::PermissionDenied,
                "Source or destination not writable",
            )
        },
    )?;
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

    // The source and destination paths are leased for the whole filesystem +
    // row transaction. The lease itself bumps and (on drop) releases the
    // per-path generation. A concurrent verify or watcher removal sees the
    // lease and skips; a newer scan submit supersedes older queued work by
    // generation.
    let _lease = crate::tasks::path_epoch::lease_paths([source_path.clone(), final_dest.clone()]);

    // Resolve-then-validate: re-read the asset record under the lease and
    // confirm its path has not moved since resolution, closing the TOCTOU
    // window between resolving the asset id and acquiring the path lease.
    let record = crate::storage::asset_store::get_asset_by_id(&asset_id)
        .or_raise(|| (ErrorKind::Database, "Failed to look up asset"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Asset not found"))?;
    if record.path != source_path.to_string_lossy() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "Asset moved concurrently",
        ));
    }

    // Copy the destination sidecar (if any) before moving the asset, so an
    // interruption never leaves an asset without its metadata. An orphan copy
    // is the worst case.
    let sidecar = source_path.with_extension("xmp");
    if sidecar.exists() {
        let new_sidecar = final_dest.with_extension("xmp");
        let bytes = fs::read(&sidecar).or_raise(|| (ErrorKind::IO, "Failed to read sidecar"))?;
        crate::process::atomic_write::write_via_temp(&new_sidecar, "sidecar", |tmp| {
            fs::write(tmp, &bytes)
        })
        .or_raise(|| (ErrorKind::IO, "Failed to copy sidecar"))?;
    }

    // Move the file on disk.
    fs::rename(&source_path, &final_dest).or_raise(|| (ErrorKind::IO, "Failed to move file"))?;

    // Remove the old sidecar after the asset moved. A missing sidecar is
    // fine; a failure is logged but does not fail the assign.
    if sidecar.exists()
        && let Err(e) = fs::remove_file(&sidecar)
    {
        log::warn!("Failed to remove old sidecar {}: {e}", sidecar.display());
    }

    // Update the asset record with the new path and album.
    let new_path = final_dest.to_string_lossy().into_owned();
    let mut updated = record.clone();
    updated.path.clone_from(&new_path);
    updated.album_id = Some(album_id);

    // Rewrite ASSET_BY_ID + ASSET_BY_PATH in one transaction (see
    // `asset_store::move_asset_mapping`), mirroring `apply_dir_move_plans`.
    // move_asset_mapping now also performs the source/target album member-row
    // re-key and aggregate deltas in the same transaction.
    asset_store::move_asset_mapping(&record.path, &new_path, &updated)
        .or_raise(|| (ErrorKind::Database, "Failed to update asset mapping"))?;

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
#[quint::operation("OP-5")]
#[allow(clippy::too_many_lines)]
fn move_album_into_album(
    album_id: ArrayString<64>,
    _target_album_id: ArrayString<64>,
    target_dir: &Path,
    on_conflict: OnConflict,
    caller: &str,
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

    // Refuse moves carrying foreign-owned shares before any filesystem mutation.
    {
        let read_txn = crate::storage::db::TREE
            .in_disk
            .begin_read()
            .or_raise(|| (ErrorKind::Database, "Failed to begin transaction"))?;
        let metadata_table = read_txn
            .open_table(crate::storage::db::METADATA_TABLE)
            .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;
        if let Some(guard) = metadata_table
            .get(album_id.as_str())
            .or_raise(|| (ErrorKind::Database, "Failed to get album"))?
            && let crate::model::metadata_record::MetadataRecord::Album(album_payload) =
                guard.value()
        {
            for share in album_payload.share_list.values() {
                if share.created_by != caller {
                    return Err(AppError::new(
                        ErrorKind::PermissionDenied,
                        "Album has shares owned by other users",
                    ));
                }
            }
        }
    }

    // Write gate: source and destination must be writable by caller.
    crate::storage::files::write_gate(caller, Some(&source_dir), Some(target_dir)).map_err(
        |_| {
            AppError::new(
                ErrorKind::PermissionDenied,
                "Source or destination not writable",
            )
        },
    )?;
    let dir_name = source_dir
        .file_name()
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album directory has no name"))?;
    let base_dest = target_dir.join(dir_name);

    // Lease the source subtree before the rename creates a window where the old
    // directory is absent but the rows still point at it. The lease itself
    // bumps and (on drop) releases the per-path generation.
    let _source_lease = crate::tasks::path_epoch::lease_subtree(source_dir.clone());

    // Resolve-then-validate under the lease.
    let reloaded = crate::storage::asset_store::get_asset_by_id(&album_id)
        .or_raise(|| (ErrorKind::Database, "Failed to look up album"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album not found"))?;
    if reloaded.path != source_dir.to_string_lossy() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "Album moved concurrently",
        ));
    }

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
        let _dest_lease = crate::tasks::path_epoch::lease_subtree(new_dir.clone());
        rewrite_dir_album_cache_prefix(&source_dir, &new_dir);
        // Update asset tables for all moved files. Path rewrites live
        // exclusively on the AssetRecords now — the metadata payload holds
        // no paths.
        update_asset_tables_after_dir_move(&source_dir, &new_dir).or_raise(|| {
            (
                ErrorKind::Database,
                "Failed to update asset tables after dir move",
            )
        })?;
    }

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

/// Test-only seam: when set to a non-negative index `k`, `apply_dir_move_plans`
/// returns an error at plan `k` without committing, simulating a failure mid-apply.
#[cfg(test)]
pub(crate) static DIR_MOVE_FAIL_AT: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(-1);

/// A single planned path rewrite produced by collecting the records whose
/// path lives under a moved directory. Built first (read-only) so the
/// apply step can commit every rewrite in one transaction.
#[derive(Clone, Debug)]
pub(crate) struct DirMovePlan {
    /// Original path key in `ASSET_BY_PATH` to remove.
    pub old_path: String,
    /// New path key to insert into `ASSET_BY_PATH`.
    pub new_path: String,
    /// Updated `AssetRecord` (path rewritten) to store under `ASSET_BY_ID`.
    pub record: AssetRecord,
}

/// Collect the set of `DirMovePlan`s for every asset whose path is `source_dir`
/// or a descendant of it. Uses the prefix range scan
/// (`asset_store::list_asset_paths_under`) instead of a full-table scan.
pub(crate) fn collect_dir_move_plans(
    source_dir: &Path,
    dest_dir: &Path,
) -> Result<Vec<DirMovePlan>, AppError> {
    use crate::storage::asset_store;

    let pairs =
        asset_store::list_asset_paths_under(&source_dir.to_string_lossy()).or_raise(|| {
            (
                ErrorKind::Database,
                "Failed to list asset paths under source",
            )
        })?;

    let mut plans = Vec::with_capacity(pairs.len());
    for (old_path, asset_id) in pairs {
        let old_path_buf = PathBuf::from(&old_path);
        let rel = old_path_buf.strip_prefix(source_dir).map_err(|_| {
            AppError::new(
                ErrorKind::Internal,
                format!("asset path not under source dir: {old_path}"),
            )
        })?;
        let new_path = dest_dir.join(rel);
        let new_path_str = new_path.to_string_lossy().into_owned();

        let record = asset_store::get_asset_by_id(asset_id.as_ref())
            .or_raise(|| (ErrorKind::Database, "Failed to read asset record"))?
            .ok_or_else(|| {
                AppError::new(
                    ErrorKind::InvalidInput,
                    format!("Asset not found: {asset_id}"),
                )
            })?;

        let mut updated = record;
        updated.path.clone_from(&new_path_str);

        plans.push(DirMovePlan {
            old_path,
            new_path: new_path_str,
            record: updated,
        });
    }

    Ok(plans)
}

/// Apply every plan in `plans` in a single write transaction. A failure on any
/// plan aborts the whole transaction (the dropped `txn` rolls back), so no
/// partial prefix rewrite is ever committed. On success all path-map removes/
/// inserts and record path updates land together.
#[quint::operation("OP-5")]
#[allow(warnings)]
pub(crate) fn apply_dir_move_plans(plans: &[DirMovePlan]) -> Result<(), AppError> {
    use crate::storage::db::{ASSET_BY_ID, ASSET_BY_PATH, TREE};

    #[cfg(test)]
    let fail_at: isize = DIR_MOVE_FAIL_AT.load(std::sync::atomic::Ordering::SeqCst);
    #[cfg(not(test))]
    let fail_at: isize = -1;
    let _ = fail_at;

    let txn = TREE
        .in_disk
        .begin_write()
        .or_raise(|| (ErrorKind::Database, "Failed to begin write transaction"))?;
    {
        let mut path_table = txn
            .open_table(ASSET_BY_PATH)
            .or_raise(|| (ErrorKind::Database, "Failed to open ASSET_BY_PATH"))?;
        let mut id_table = txn
            .open_table(ASSET_BY_ID)
            .or_raise(|| (ErrorKind::Database, "Failed to open ASSET_BY_ID"))?;

        for (i, plan) in plans.iter().enumerate() {
            let _ = i;
            #[cfg(test)]
            if fail_at >= 0 && i.cast_signed() == fail_at {
                return Err(AppError::new(
                    ErrorKind::Database,
                    format!("injected failure at plan {i}"),
                ));
            }

            path_table
                .remove(plan.old_path.as_str())
                .or_raise(|| (ErrorKind::Database, "Failed to remove old path mapping"))?;
            path_table
                .insert(plan.new_path.as_str(), &*plan.record.asset_id)
                .or_raise(|| (ErrorKind::Database, "Failed to add new path mapping"))?;

            let json = serde_json::to_string(&plan.record)
                .or_raise(|| (ErrorKind::Serialization, "Failed to serialize AssetRecord"))?;
            id_table
                .insert(&*plan.record.asset_id, json.as_str())
                .or_raise(|| (ErrorKind::Database, "Failed to update asset record"))?;
        }
    }
    txn.commit()
        .or_raise(|| (ErrorKind::Database, "Failed to commit dir move"))?;

    Ok(())
}

/// Update asset tables after a directory move.
/// For each asset whose path starts with `source_dir`, update it
/// to the corresponding path under `dest_dir`. Collects all planned rewrites
/// first, then applies them in a single write transaction (atomic).
pub(crate) fn update_asset_tables_after_dir_move(
    source_dir: &Path,
    dest_dir: &Path,
) -> Result<(), AppError> {
    let plans = collect_dir_move_plans(source_dir, dest_dir)?;
    apply_dir_move_plans(&plans)
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

    /// The per-asset move writes `ASSET_BY_ID` and then `ASSET_BY_PATH` in
    /// separate transactions. This test constructs the torn window directly
    /// (filesystem move + id-table write, path rewrite not yet landed), then
    /// runs the next scan's flush and the sweep: the stale source path is
    /// pruned and every remaining path key resolves to a record naming that
    /// path. It pins OP-4's `aspectsN5SplitWindowTest` heal half. Honest
    /// limit: `move_asset_into_album` has no failure-injection seam, so the
    /// window is built by hand; and the heal allocates a new asset id for the
    /// moved file (N5 identity loss on a mid-move crash remains open) — only
    /// cross-table coherence is restored.
    #[test]
    fn move_split_window_scan_and_sweep_heal_cross_table() {
        use crate::model::abstract_data::AbstractData;
        use crate::process::hash::blake3_hasher;
        use crate::storage::asset_store;
        use crate::tasks::batcher::flush_tree::{flush_tables, sweep_remove_batch};
        use crate::tests::bootstrap;
        use snapfab::{PhotoSpec, generate_batch};
        use std::fs::File;

        let _guard = bootstrap::lock();
        let _ = &*bootstrap::TEST_ENV;
        bootstrap::reset_backend_state();

        let root = bootstrap::test_image_home();
        let src_dir = root.join("move_window/a");
        let dst_dir = root.join("move_window/b");
        std::fs::create_dir_all(&src_dir).expect("create source dir");
        std::fs::create_dir_all(&dst_dir).expect("create destination dir");
        let source = src_dir.join("photo.jpg");
        let dest = dst_dir.join("photo.jpg");
        generate_batch(&[PhotoSpec {
            output: Some(source.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate move-window photo");

        let hash = blake3_hasher(File::open(&source).expect("open source")).expect("hash source");
        flush_tables(
            &[AbstractData::new(&source, hash).expect("build source data")],
            &[],
        )
        .expect("flush tables");
        let old_id = asset_store::get_asset_id_by_path(&source.to_string_lossy())
            .expect("read source path")
            .expect("source row exists");

        // The move's filesystem half plus the ASSET_BY_ID write, before the
        // ASSET_BY_PATH rewrite: the exact N5 split window.
        std::fs::rename(&source, &dest).expect("move file");
        let mut moved = asset_store::get_asset_by_id(old_id.as_str())
            .expect("read moved record")
            .expect("moved record exists");
        moved.path = dest.to_string_lossy().into_owned();
        asset_store::put_asset_by_id(&moved).expect("id-table write");
        assert_eq!(
            asset_store::get_asset_id_by_path(&source.to_string_lossy())
                .expect("read stale source mapping"),
            Some(old_id),
            "window premise: the source path mapping still exists"
        );
        assert!(
            asset_store::get_asset_id_by_path(&dest.to_string_lossy())
                .expect("read destination mapping")
                .is_none(),
            "window premise: the destination has no path mapping yet"
        );

        // The next scan flushes the file at its new path, then the sweep
        // prunes the stale source entry.
        flush_tables(
            &[AbstractData::new(&dest, hash).expect("build destination data")],
            &[],
        )
        .expect("flush tables");
        sweep_remove_batch(std::slice::from_ref(&source)).expect("sweep remove batch");

        let new_id = asset_store::get_asset_id_by_path(&dest.to_string_lossy())
            .expect("read destination path")
            .expect("destination row exists");
        let new_record = asset_store::get_asset_by_id(new_id.as_str())
            .expect("read destination record")
            .expect("destination record exists");
        assert_eq!(
            new_record.path,
            dest.to_string_lossy(),
            "destination mapping must point at a record naming the destination"
        );
        assert!(
            asset_store::get_asset_id_by_path(&source.to_string_lossy())
                .expect("read swept source mapping")
                .is_none(),
            "the sweep must remove the stale source mapping"
        );
        assert!(
            asset_store::get_asset_by_id(old_id.as_str())
                .expect("read stale record")
                .is_none(),
            "the sweep must remove the split record once its mapping is stale"
        );

        // No path key may point at a missing record or a record describing
        // another path.
        let rows = asset_store::list_asset_paths_under(&root.to_string_lossy())
            .expect("list path rows under root");
        for (path, id) in rows {
            let record = asset_store::get_asset_by_id(id.as_str())
                .expect("read mapped record")
                .unwrap_or_else(|| panic!("path mapping {path} points at a missing record"));
            assert_eq!(
                record.path, path,
                "path mapping {path} and record path must agree"
            );
        }

        std::fs::remove_dir_all(root.join("move_window")).ok();
        bootstrap::reset_backend_state();
    }

    #[test]
    fn update_asset_tables_after_dir_move_preserves_asset_id() {
        use crate::model::asset::{AssetKind, AssetRecord};
        use crate::storage::asset_store;
        use crate::storage::db::{ASSET_BY_ID, ASSET_BY_PATH};
        use crate::tests::bootstrap;
        use redb::ReadableTable;

        let _guard = bootstrap::lock();
        let _ = &*bootstrap::TEST_ENV;

        // Ensure tables exist.
        let txn = crate::storage::db::TREE
            .in_disk
            .begin_write()
            .expect("begin write txn for table creation");
        txn.open_table(ASSET_BY_PATH).expect("create ASSET_BY_PATH");
        txn.open_table(ASSET_BY_ID).expect("create ASSET_BY_ID");
        txn.commit().expect("commit table creation");

        // Clear both tables.
        for table_def in [ASSET_BY_PATH, ASSET_BY_ID] {
            let txn = crate::storage::db::TREE
                .in_disk
                .begin_write()
                .expect("begin write for clear");
            {
                let t = txn.open_table(table_def).expect("open table for clear");
                let keys: Vec<String> = t
                    .iter()
                    .expect("iterate")
                    .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                    .collect();
                drop(t);
                let mut t = txn.open_table(table_def).expect("reopen table for clear");
                for key in &keys {
                    t.remove(key.as_str()).expect("remove key");
                }
            }
            txn.commit().expect("commit clear");
        }

        let record = AssetRecord::new_media(
            AssetKind::Image,
            "/src/dir/photo.jpg".into(),
            None,
            10,
            "jpg".into(),
            0,
        );
        let id = record.asset_id;
        asset_store::insert_asset(&record).expect("insert test asset");

        super::update_asset_tables_after_dir_move(Path::new("/src/dir"), Path::new("/dst/dir"))
            .expect("dir move rewrite");

        assert!(
            asset_store::get_asset_id_by_path("/src/dir/photo.jpg")
                .expect("read old path")
                .is_none(),
            "old path mapping must be removed"
        );
        assert_eq!(
            asset_store::get_asset_id_by_path("/dst/dir/photo.jpg").expect("read new path"),
            Some(id),
            "new path mapping must carry the same asset id"
        );
        let updated = asset_store::get_asset_by_id(id.as_ref())
            .expect("read record")
            .expect("record present");
        assert_eq!(updated.asset_id, id, "asset id must be preserved");
        assert_eq!(updated.path, "/dst/dir/photo.jpg", "path must be rewritten");

        // Cleanup.
        for table_def in [ASSET_BY_PATH, ASSET_BY_ID] {
            let txn = crate::storage::db::TREE
                .in_disk
                .begin_write()
                .expect("begin write for cleanup");
            {
                let t = txn.open_table(table_def).expect("open table for cleanup");
                let keys: Vec<String> = t
                    .iter()
                    .expect("iterate")
                    .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                    .collect();
                drop(t);
                let mut t = txn.open_table(table_def).expect("reopen table for cleanup");
                for key in &keys {
                    t.remove(key.as_str()).expect("remove key");
                }
            }
            txn.commit().expect("commit cleanup");
        }
    }

    #[test]
    fn update_asset_tables_after_dir_move_is_all_or_nothing() {
        use crate::model::asset::{AssetKind, AssetRecord};
        use crate::storage::asset_store;
        use crate::storage::db::{ASSET_BY_ID, ASSET_BY_PATH};
        use crate::tests::bootstrap;
        use redb::ReadableTable;
        use std::sync::atomic::Ordering;

        let _guard = bootstrap::lock();
        let _ = &*bootstrap::TEST_ENV;

        // Ensure tables exist.
        let txn = crate::storage::db::TREE
            .in_disk
            .begin_write()
            .expect("begin write txn for table creation");
        txn.open_table(ASSET_BY_PATH).expect("create ASSET_BY_PATH");
        txn.open_table(ASSET_BY_ID).expect("create ASSET_BY_ID");
        txn.commit().expect("commit table creation");

        // Clear both tables.
        for table_def in [ASSET_BY_PATH, ASSET_BY_ID] {
            let txn = crate::storage::db::TREE
                .in_disk
                .begin_write()
                .expect("begin write for clear");
            {
                let t = txn.open_table(table_def).expect("open table for clear");
                let keys: Vec<String> = t
                    .iter()
                    .expect("iterate")
                    .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                    .collect();
                drop(t);
                let mut t = txn.open_table(table_def).expect("reopen table for clear");
                for key in &keys {
                    t.remove(key.as_str()).expect("remove key");
                }
            }
            txn.commit().expect("commit clear");
        }

        let (src, dst) = (Path::new("/src/dir"), Path::new("/dst/dir"));

        let mut ids = Vec::new();
        for i in 0..3 {
            let record = AssetRecord::new_media(
                AssetKind::Image,
                format!("/src/dir/photo{i}.jpg"),
                None,
                10,
                "jpg".into(),
                0,
            );
            ids.push(record.asset_id);
            asset_store::insert_asset(&record).expect("insert test asset");
        }

        let plans = super::collect_dir_move_plans(src, dst).expect("collect plans");
        assert_eq!(plans.len(), 3);

        // Inject a failure at plan index 1.
        super::DIR_MOVE_FAIL_AT.store(1, Ordering::SeqCst);
        let res = super::apply_dir_move_plans(&plans);
        super::DIR_MOVE_FAIL_AT.store(-1, Ordering::SeqCst);
        assert!(res.is_err(), "apply must fail when a plan fails");

        // All-or-nothing: old prefix fully intact, no new-prefix rows.
        for i in 0..3 {
            let old = format!("/src/dir/photo{i}.jpg");
            assert!(
                asset_store::get_asset_id_by_path(&old)
                    .expect("read old path")
                    .is_some(),
                "old path must remain: {old}"
            );
        }
        let new_rows =
            asset_store::list_asset_paths_under(&dst.to_string_lossy()).expect("list new");
        assert!(
            new_rows.is_empty(),
            "no new-prefix rows must exist: {new_rows:?}"
        );
        for id in &ids {
            let rec = asset_store::get_asset_by_id(id.as_ref())
                .expect("read record")
                .expect("record present");
            assert!(
                rec.path.starts_with("/src/dir/"),
                "record path unchanged: {}",
                rec.path
            );
        }

        // Cleanup.
        for table_def in [ASSET_BY_PATH, ASSET_BY_ID] {
            let txn = crate::storage::db::TREE
                .in_disk
                .begin_write()
                .expect("begin write for cleanup");
            {
                let t = txn.open_table(table_def).expect("open table for cleanup");
                let keys: Vec<String> = t
                    .iter()
                    .expect("iterate")
                    .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                    .collect();
                drop(t);
                let mut t = txn.open_table(table_def).expect("reopen table for cleanup");
                for key in &keys {
                    t.remove(key.as_str()).expect("remove key");
                }
            }
            txn.commit().expect("commit cleanup");
        }
    }

    fn read_metadata_payload(
        asset_id: &str,
    ) -> Option<crate::model::metadata_record::MetadataRecord> {
        let txn = crate::storage::db::TREE
            .in_disk
            .begin_read()
            .expect("begin read txn for metadata");
        let table = txn
            .open_table(crate::storage::db::METADATA_TABLE)
            .expect("open METADATA_TABLE");
        table.get(asset_id).ok().flatten().map(|g| g.value())
    }

    #[test]
    fn move_preserves_destination_sidecar_and_removes_source_sidecar() {
        use crate::tests::bootstrap::{lock, setup_image_home, test_image_home};
        use crate::tests::fixtures::auth_cookie;
        use crate::tests::fixtures::authz::create_album;
        use rocket::http::{ContentType, Status};
        use serde_json::json;

        let _guard = lock();
        let client = setup_image_home();
        let (source_album_id, asset_id) = create_album(&client, "/writes/photo.jpg");

        let resp = client
            .post("/post/create_dir_album")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(json!({ "parentAlbumId": source_album_id, "name": "dest" }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "create dest album: {resp:?}");
        let target_album_id = resp.into_string().expect("album id").trim().to_string();

        let image_home = test_image_home();
        let source_dir = image_home.join("writes");
        let dest_dir = image_home.join("writes/dest");
        let source_photo = source_dir.join("photo.jpg");
        let source_sidecar = source_dir.join("photo.xmp");
        let dest_photo = dest_dir.join("photo.jpg");
        let dest_sidecar = dest_dir.join("photo.xmp");

        let sidecar_bytes = b"<xmp>tags/rating</xmp>";
        std::fs::write(&source_sidecar, sidecar_bytes).expect("write source sidecar");

        let before_payload = read_metadata_payload(&asset_id);

        let resp = client
            .put("/put/assign_album")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(
                json!({
                    "assetId": asset_id,
                    "albumId": target_album_id,
                    "onConflict": "rename"
                })
                .to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "assign: {resp:?}");

        assert!(dest_photo.exists(), "destination asset must exist");
        assert!(!source_photo.exists(), "source asset must be gone");
        assert!(dest_sidecar.exists(), "destination sidecar must exist");
        assert!(!source_sidecar.exists(), "source sidecar must be gone");
        assert_eq!(
            std::fs::read(&dest_sidecar).expect("read dest sidecar"),
            sidecar_bytes,
            "destination sidecar bytes must match"
        );

        let after_payload = read_metadata_payload(&asset_id);
        assert_eq!(
            before_payload, after_payload,
            "metadata payload must be undisturbed by a successful move"
        );

        std::fs::remove_dir_all(image_home.join("writes")).ok();
        crate::tests::bootstrap::reset_backend_state();
    }

    #[test]
    fn move_crash_heal_reindexes_from_destination_sidecar() {
        // Construct the crash residue directly: file + sidecar present at
        // destination, DB left in the torn state (id table updated to dest
        // path, path mapping still stale at source). Then drive a rescan/scan
        // apply of the destination dir and assert the healed row's metadata
        // payload carries the sidecar's user metadata.
        use crate::model::abstract_data::AbstractData;
        use crate::process::hash::blake3_hasher;
        use crate::storage::asset_store;
        use crate::tasks::batcher::flush_tree::{flush_tables, sweep_remove_batch};
        use crate::tests::bootstrap::{lock, reset_backend_state, test_image_home};
        use snapfab::{PhotoSpec, generate_batch};
        use std::fs::File;

        let _guard = lock();
        let _ = &*crate::tests::bootstrap::TEST_ENV;
        reset_backend_state();

        let root = test_image_home();
        let src_dir = root.join("move_crash/a");
        let dst_dir = root.join("move_crash/b");
        std::fs::create_dir_all(&src_dir).expect("create source dir");
        std::fs::create_dir_all(&dst_dir).expect("create destination dir");
        let source = src_dir.join("photo.jpg");
        let dest = dst_dir.join("photo.jpg");
        let dest_sidecar = dst_dir.join("photo.xmp");

        generate_batch(&[PhotoSpec {
            output: Some(source.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate photo");

        // Seed a sidecar carrying user metadata (tags/rating) at the destination,
        // mirroring copy-before-rename. A valid XMP packet so the scanner's
        // sidecar read yields non-default metadata.
        let sidecar_bytes = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xmp="http://ns.adobe.com/xap/1.0/">
<dc:subject><rdf:Bag><rdf:li>heal-tag</rdf:li></rdf:Bag></dc:subject>
<xmp:Rating>5</xmp:Rating>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>"#;
        std::fs::write(&dest_sidecar, sidecar_bytes).expect("write dest sidecar");

        let hash = blake3_hasher(File::open(&source).expect("open source")).expect("hash source");
        // Index the source first (no sidecar metadata on source).
        let src_data = AbstractData::new(&source, hash).expect("build source data");
        std::fs::rename(&source, &dest).expect("move file");

        // Destination AbstractData carries the sidecar's user metadata (as a
        // scan would read it from the .xmp).
        let xmp = crate::process::xmp::extract_xmp_data_from_file(&dest);
        assert!(!xmp.tags.is_empty(), "sidecar must yield tags");
        let mut dest_data = AbstractData::new(&dest, hash).expect("build dest data");
        if let AbstractData::Image(img) = &mut dest_data {
            img.object.tags = xmp.tags.clone();
            img.object.rating = xmp.rating;
        }

        // Torn DB state: id table updated to dest path, path mapping still at
        // source (the N5 split window).
        flush_tables(&[src_data], &[]).expect("flush tables");
        let old_id = asset_store::get_asset_id_by_path(&source.to_string_lossy())
            .expect("read source path")
            .expect("source row exists");
        let mut moved = asset_store::get_asset_by_id(old_id.as_str())
            .expect("read moved record")
            .expect("moved record exists");
        moved.path = dest.to_string_lossy().into_owned();
        asset_store::put_asset_by_id(&moved).expect("id-table write");

        // Drive a rescan of the destination: flush the destination file, then
        // sweep the stale source mapping.
        flush_tables(&[dest_data], &[]).expect("flush tables");
        sweep_remove_batch(std::slice::from_ref(&source)).expect("sweep remove batch");

        // Healed row at destination must carry the sidecar's user metadata.
        let new_id = asset_store::get_asset_id_by_path(&dest.to_string_lossy())
            .expect("read dest path")
            .expect("dest row exists after heal");
        let new_record = asset_store::get_asset_by_id(new_id.as_str())
            .expect("read dest record")
            .expect("dest record present");
        assert_eq!(new_record.path, dest.to_string_lossy());

        let txn = crate::storage::db::TREE
            .in_disk
            .begin_read()
            .expect("begin read");
        let table = txn
            .open_table(crate::storage::db::METADATA_TABLE)
            .expect("open meta");
        let payload = table
            .get(new_id.as_str())
            .expect("read payload")
            .expect("payload present")
            .value();
        match payload {
            crate::model::metadata_record::MetadataRecord::Image(p) => {
                assert!(!p.tags.is_empty(), "healed payload tags must be non-empty");
                assert_eq!(
                    p.rating,
                    Some(5),
                    "healed payload rating must come from sidecar"
                );
            }
            _ => panic!("expected image payload"),
        }

        drop(txn);
        std::fs::remove_dir_all(root.join("move_crash")).ok();
        reset_backend_state();
    }

    /// A directory move rewrites paths only: membership is keyed by `album_id`,
    /// so the member row and aggregates must be untouched and remain exact.
    #[test]
    fn dir_move_preserves_album_membership_and_aggregates() {
        use crate::model::abstract_data::AbstractData;
        use crate::process::hash::blake3_hasher;
        use crate::storage::asset_store;
        use crate::tests::bootstrap::{
            TEST_ENV, album_member_ids, assert_album_aggregates_exact, lock, read_album_payload,
            reset_backend_state, test_image_home,
        };
        use std::fs::File;

        let _guard = lock();
        let _ = &*TEST_ENV;
        reset_backend_state();

        let root = test_image_home().join("dir_move_membership");
        let src = root.join("album");
        let dst = root.join("moved");
        std::fs::create_dir_all(&src).expect("create album dir");
        let album_id =
            crate::process::dir_album::get_or_create_dir_album(src.clone()).expect("create album");

        let path = src.join("photo.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(path.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate photo");
        let hash = blake3_hasher(File::open(&path).expect("open photo")).expect("hash photo");
        let mut data = AbstractData::new(&path, hash).expect("build photo data");
        data.set_album(Some(album_id));
        crate::tasks::batcher::flush_tree::flush_tables(&[data], &[]).expect("flush album member");

        let member_id = asset_store::get_asset_id_by_path(&path.to_string_lossy())
            .expect("read member path")
            .expect("member path exists");
        assert_eq!(
            album_member_ids(album_id.as_str()),
            vec![member_id.to_string()]
        );

        std::fs::rename(&src, &dst).expect("move album directory");
        let plans = collect_dir_move_plans(&src, &dst).expect("collect dir move plans");
        apply_dir_move_plans(&plans).expect("apply dir move plans");

        let moved = asset_store::get_asset_by_id(member_id.as_str())
            .expect("read moved member")
            .expect("moved member exists");
        assert!(
            moved.path.starts_with(dst.to_string_lossy().as_ref()),
            "record path must live under the destination: {}",
            moved.path
        );
        assert_eq!(
            album_member_ids(album_id.as_str()),
            vec![member_id.to_string()],
            "a path-only move must not re-key the member row"
        );
        let payload = read_album_payload(album_id.as_str());
        assert_eq!(payload.item_count, 1);
        assert_album_aggregates_exact(album_id.as_str());

        std::fs::remove_dir_all(&root).ok();
        reset_backend_state();
    }
}
