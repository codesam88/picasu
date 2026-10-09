use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::openapi_components::Unauthorized;
use crate::process::sidecar_edit::{EditedItem, commit_metadata_edits};
use crate::process::transitor::index_to_asset_id;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::open_tree_snapshot_table;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use anyhow::Result;
use arrayvec::ArrayString;
use rocket::serde::{Deserialize, Serialize, json::Json};
use std::collections::HashSet;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct EditFlagsData {
    index_array: Vec<usize>,
    timestamp: i64,
    #[serde(default)]
    is_trashed: Option<bool>,
}

/// Set or clear the trash flag on the listed assets.
///
/// `indexArray` addresses the assets in the snapshot named by `timestamp`, and
/// a sent `isTrashed` value lands on each asset's own record *and* on its XMP
/// sidecar as the `picasu:Trashed` marker: the record is a cache, so without
/// the marker a rebuild from the filesystem would bring trashed files back as
/// normal assets. A restore removes the marker. The sidecar is written before
/// the record, and a write that cannot land fails the request without storing
/// anything. Every album that lost or regained a member is refreshed in the
/// background, so its counts may still be catching up when the call returns.
///
/// Corner cases: the removed `isFavorite` and `isArchived` keys are still
/// accepted and ignored rather than rejected, and an absent `isTrashed` leaves
/// every flag as it was. An index whose asset record no longer exists is
/// skipped and still answers 200.
///
/// Errors: 400 malformed body — 401 missing or invalid admin credentials; share
/// tokens are not accepted — 405 read-only mode — 500 unknown snapshot,
/// out-of-range index, or storage failure.
#[utoipa::path(
        tag = "assets",
        request_body = EditFlagsData,
        responses(
            (status = 200, description = "Flags updated"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/edit_flags", format = "json", data = "<json_data>")]
pub async fn edit_flags(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<EditFlagsData>,
) -> AppResult<Json<()>> {
    let _ = auth?;
    let _ = read_only_mode?;

    // The flag goes to disk as well as to the record, so the edit runs through
    // the shared write-then-store contract: every sidecar lands first, and only
    // then is every record stored. Nothing is half-applied.
    let is_trashed = json_data.is_trashed;

    let affected_album_ids =
        tokio::task::spawn_blocking(move || -> Result<HashSet<ArrayString<64>>, AppError> {
            let tree_snapshot = open_tree_snapshot_table(json_data.timestamp)
                .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot"))?;

            let mut affected_album_ids = HashSet::new();
            let mut items: Vec<EditedItem> = Vec::new();

            for &index in &json_data.index_array {
                let asset_id = index_to_asset_id(&tree_snapshot, index).or_raise(|| {
                    (
                        ErrorKind::Database,
                        format!("Failed to get asset_id for index {index}"),
                    )
                })?;

                if let Some(abstract_data) =
                    crate::process::transitor::compose_by_asset_id(&asset_id)
                        .or_raise(|| (ErrorKind::Database, "Failed to get data"))?
                {
                    // A trash change is the only kind that moves members between
                    // albums' visible sets.
                    if is_trashed.is_some()
                        && let Some(album_id) = abstract_data.album()
                    {
                        affected_album_ids.insert(album_id);
                    }

                    // The composed view still holds the pre-edit flag. The
                    // sidecar is written from this view, so the new value has
                    // to be applied here — writing it as composed would record
                    // the old state.
                    if let Some(trashed) = is_trashed {
                        let mut edited = abstract_data;
                        edited.set_trashed(trashed);
                        items.push(EditedItem {
                            asset_id,
                            data: edited,
                        });
                    }
                }
            }

            commit_metadata_edits(&items, |asset_id: &ArrayString<64>, data: &AbstractData| {
                crate::process::transitor::store_metadata_record(asset_id, data, is_trashed)
                    .or_raise(|| (ErrorKind::Database, "Failed to store metadata"))
            })?;

            Ok(affected_album_ids)
        })
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    // Drain pending flush before rebuilding the in-memory tree.
    let _ = BATCH_COORDINATOR
        .execute_batch_waiting(FlushTreeTask::insert(vec![]))
        .await;
    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;

    // After memory update, trigger album self-update
    if !affected_album_ids.is_empty() {
        for album_id in affected_album_ids {
            BATCH_COORDINATOR.execute_detached(AlbumSelfUpdateTask::new(album_id));
        }
    }

    Ok(Json(()))
}

#[cfg(test)]
mod tests {
    use super::EditFlagsData;

    /// The request body only carries the trash flag now: favorite/archived
    /// were removed from the payload, so a re-serialized body must not
    /// mention them while `isTrashed` stays intact.
    #[test]
    fn serialized_body_only_carries_trash_flag() {
        let body: EditFlagsData =
            serde_json::from_str(r#"{"indexArray":[3],"timestamp":1712,"isTrashed":true}"#)
                .expect("trash-only body must deserialize");
        let value = serde_json::to_value(body).expect("EditFlagsData must serialize");
        let map = value
            .as_object()
            .expect("EditFlagsData serializes to an object");

        assert!(
            !map.contains_key("isFavorite"),
            "isFavorite must be removed"
        );
        assert!(
            !map.contains_key("isArchived"),
            "isArchived must be removed"
        );
        assert_eq!(
            map.get("isTrashed"),
            Some(&serde_json::Value::Bool(true)),
            "trash flag must be retained"
        );
    }

    #[test]
    fn deserializes_body_without_flags() {
        let body: EditFlagsData = serde_json::from_str(r#"{"indexArray":[0,1],"timestamp":7}"#)
            .expect("flag-less body must deserialize");
        assert_eq!(body.index_array, vec![0, 1]);
        assert_eq!(body.timestamp, 7);
        assert_eq!(body.is_trashed, None, "absent trash flag stays None");
    }

    /// A client that has not caught up yet may still send the removed keys;
    /// they are ignored rather than rejected, and never resurrect a flag.
    #[test]
    fn legacy_favorite_and_archived_keys_are_ignored() {
        let body: EditFlagsData = serde_json::from_str(
            r#"{"indexArray":[],"timestamp":7,"isFavorite":true,"isArchived":true}"#,
        )
        .expect("body with legacy keys must deserialize");
        let value = serde_json::to_value(&body).expect("EditFlagsData must serialize");
        let map = value
            .as_object()
            .expect("EditFlagsData serializes to an object");

        assert!(!map.contains_key("isFavorite"));
        assert!(!map.contains_key("isArchived"));
        assert_eq!(body.is_trashed, None);
    }
}
