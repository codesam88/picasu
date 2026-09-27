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

/// Full metadata detail for a single asset, composed from its identity and its stored metadata payload.
///
/// This is the detail-side counterpart of `get-data`: list rows only carry
/// lean identity fields (tags / EXIF / description / rating are stripped in
/// Phase 14), so the sidebar, detail view, and edit prefill fetch the full
/// `AbstractData` view here on demand. The wire shape is unchanged:
/// identity fields come from the record, metadata fields from the payload.
///
/// The response carries the `exifVec` map and, for images, `furtherMetadata` —
/// the read-only bucket of metadata the app does not model, keyed `Group:Tag`
/// (`IPTC:By-line`, `XMP-xmp:CreatorTool`). The bucket is the one field here
/// with no edit path in the API: nothing reads it into an app field and nothing
/// writes it back. Which keys reach it, and which `ExifTool` groups are excluded
/// from it, is documented on `process::xmp::map_further_fields`.
///
/// Auth and share parity follow `get-data`: a `GuardTimestamp` bearer token
/// (prefetch token) is required, and when the token resolves to a share with
/// `show_metadata: false` the metadata fields are cleared before responding so
/// a share that hides metadata cannot leak it through this route.
#[utoipa::path(
        tag = "assets",
        responses(
            (
                status = 200,
                description = "Full metadata record for the asset: `exifVec` and the read-only \
                              `furtherMetadata` bucket, alongside the app's own fields"
            ),
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
