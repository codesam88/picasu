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

/// Full metadata detail for a single asset, composed at the edge from the
/// asset's identity `AssetRecord` and its stored `METADATA_TABLE` payload.
///
/// This is the detail-side counterpart of `get-data`: list rows only carry
/// lean identity fields (tags / EXIF / description / rating are stripped in
/// Phase 14), so the sidebar, detail view, and edit prefill fetch the full
/// `AbstractData` view here on demand. The wire shape is unchanged:
/// identity fields come from the record, metadata fields from the payload.
///
/// Auth and share parity follow `get-data`: a `GuardTimestamp` bearer token
/// (prefetch token) is required, and when the token resolves to a share with
/// `show_metadata: false` the metadata fields are cleared before responding so
/// a share that hides metadata cannot leak it through this route.
#[utoipa::path(
        get,
        path = "/get/metadata/{asset_id}",
        tag = "assets",
        responses(
            (status = 200, description = "Full metadata record for the asset"),
            (status = 404, description = "Unknown asset_id"),
            (status = 401, response = Unauthorized),
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
