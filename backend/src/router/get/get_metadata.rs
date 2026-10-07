// src/router/get/get_metadata.rs
use arrayvec::ArrayString;
use rocket::get;
use rocket::serde::json::Json;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::openapi_components::Unauthorized;
use crate::process::resolve_show_download_and_metadata;
use crate::process::transitor::clear_abstract_data_metadata;
use crate::process::transitor::compose_by_asset_id;
use crate::router::auth::GuardTimestamp;
use crate::router::{AppResult, GuardResult};

/// Serve the full metadata of a single asset.
///
/// List rows carry only lean identity fields, so the detail view, sidebar and
/// edit prefill fetch the complete asset here. The wire shape is unchanged:
/// identity fields come from the asset's identity record, metadata fields from
/// its stored payload.
///
/// Corner cases: `timestamp` is required and must equal the prefetch token's
/// `timestamp` claim. When the token resolves to a share with
/// `show_metadata: false`, the metadata fields — including the stored path —
/// are cleared before responding, so a share that hides metadata cannot leak it
/// here.
///
/// Errors: 400 invalid `asset_id` — 401 missing, invalid, or mismatched
/// prefetch token — 404 unknown `asset_id` — 500 the asset record could not be
/// composed.
#[utoipa::path(
        tag = "assets",
        params(
            ("asset_id" = &str, Path, description = "Asset to read the full metadata for"),
            ("timestamp" = i64, Query, description = "Must equal the prefetch token's timestamp claim"),
        ),
        responses(
            (status = 200, description = "Full metadata record for the asset"),
            (status = 404, description = "Unknown asset_id"),
            (status = 401, response = Unauthorized),
            (status = 400, description = "Invalid input"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/get/metadata/<asset_id>?<timestamp>")]
pub async fn get_metadata(
    guard_timestamp: GuardResult<GuardTimestamp>,
    asset_id: &str,
    #[allow(unused_variables)] timestamp: i64,
) -> AppResult<Json<AbstractData>> {
    let guard_timestamp = guard_timestamp?;
    let asset_id = asset_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<Json<AbstractData>, AppError> {
        let resolved_share_opt = guard_timestamp.claims.resolved_share_opt;
        let (_, show_metadata) = resolve_show_download_and_metadata(resolved_share_opt);

        let asset_id = ArrayString::<64>::from(asset_id.as_str()).map_err(|_| {
            AppError::new(
                ErrorKind::InvalidInput,
                format!("Invalid asset_id: {asset_id}"),
            )
        })?;

        // 404 when the identity record is missing; otherwise compose the
        // record with its stored payload (payload defaults when absent).
        let mut abstract_data = compose_by_asset_id(&asset_id)
            .or_raise(|| (ErrorKind::Database, "Failed to compose record"))?
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Record not found"))?;

        // Same clearing rules the list path applies: strip metadata fields
        // (including the stored path) when the share hides them.
        clear_abstract_data_metadata(&mut abstract_data, show_metadata);
        Ok(Json(abstract_data))
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?
}
