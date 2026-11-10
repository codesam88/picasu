use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::openapi_components::Unauthorized;
use crate::process::misc::{apply_rotation, generate_phash, generate_thumbhash};
use crate::process::thumbnail::generate_thumbnail_for_image;
use crate::process::xmp::extract_xmp_data_from_file;
use crate::process::xmp_write::write_rotation_for;
use crate::router::{AppResult, GuardResult};
use crate::storage::asset_store;
use crate::storage::files::write_gate;
use crate::tasks::batcher::flush_tree::FlushTreeTask;

use crate::router::auth::GuardMutate;
use crate::router::auth::GuardUser;
use crate::tasks::BATCH_COORDINATOR;
use anyhow::Result;
// use anyhow::anyhow;
use arrayvec::ArrayString;
use log::info;
use rocket::serde::{Deserialize, Serialize, json::Json};

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct RotateImageRequest {
    /// Asset ID of the image to rotate
    pub asset_id: String,
}

/// Rotate an image 90° counter-clockwise and rebuild its derivatives.
///
/// The compressed JPEG is loaded, rotated and written back, `width` and
/// `height` are swapped, and `thumbhash` and `phash` are recomputed. The
/// accumulated rotation (90°, 180°, 270°, then back to 0°) is persisted to the
/// asset's XMP sidecar as `picasu:Rotation`, which is the source of truth a
/// later reindex reads to reproduce this display state. Each parent album's
/// `updatedAt` is bumped before the records are flushed.
///
/// Corner cases: only the compressed JPEG is re-rendered — the original file
/// and its EXIF data are left as they were — so the rotation is a display-level
/// transform that accumulates across repeated calls. The original content hash
/// is not recomputed.
///
/// Errors: 400 an `asset_id` longer than 64 bytes, or an album or video — 401
/// missing or invalid credentials; share tokens are not accepted — 404
/// `asset_id` names no asset record — 405 read-only mode — 500 decode,
/// thumbnail write, XMP sidecar write, or storage failure.
#[utoipa::path(
        tag = "assets",
        request_body = RotateImageRequest,
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
            (status = 200, description = "Image rotated"),
            (status = 400, description = "Invalid input"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
            (status = 404, description = "Not found"),
        )
    )
]
#[put("/put/rotate-image", data = "<request>")]
pub async fn rotate_image(
    auth: GuardResult<GuardUser>,
    mutation: GuardResult<GuardMutate>,
    request: Json<RotateImageRequest>,
) -> AppResult<()> {
    let GuardUser { claims } = auth?;
    let _ = mutation?;
    let (caller, _) = crate::router::put::users_password::caller_identity(&claims);

    let _asset_id = ArrayString::<64>::from(&request.asset_id)
        .map_err(|_| AppError::new(ErrorKind::InvalidInput, "Invalid asset_id length or format"))?;

    if request.asset_id.is_empty() || request.asset_id.len() > 64 {
        return Err(AppError::new(ErrorKind::InvalidInput, "Invalid asset_id"));
    }

    let abstract_data =
        tokio::task::spawn_blocking(move || -> Result<Vec<AbstractData>, AppError> {
            let abstract_data = asset_store::lookup_abstract_data_by_asset_id(&request.asset_id)
                .or_raise(|| (ErrorKind::Database, "Failed to fetch DB record"))?
                .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Asset not found"))?;

            if let Some(path) = abstract_data.visibility_path() {
                write_gate(&caller, Some(&path), None).map_err(|_| {
                    AppError::new(ErrorKind::PermissionDenied, "Asset not writable")
                })?;
            }

            let mut abstract_data = abstract_data;

            // Only rotate images, not videos or albums
            if !matches!(abstract_data, AbstractData::Image(_)) {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    "Only images can be rotated",
                ));
            }

            // The sidecar is the rotation source of truth; each call adds one
            // quarter turn counter-clockwise.
            let current_rotation =
                extract_xmp_data_from_file(&abstract_data.source_path()).rotation;
            let next_rotation = (current_rotation + 90) % 360;

            // Load the compressed image (not the original)
            let compressed_path = abstract_data.compressed_path();
            let dyn_img = image::open(&compressed_path).map_err(|e| {
                AppError::new(
                    ErrorKind::IO,
                    format!(
                        "Failed to load compressed image: {} ({e})",
                        compressed_path.display()
                    ),
                )
            })?;

            // Rotate counter-clockwise and swap the display dimensions.
            let dyn_img = apply_rotation(&mut abstract_data, dyn_img, 90);

            // Persist the rotation before the thumbnail and DB: the sidecar is
            // the source of truth, so an interruption after this point leaves a
            // present sidecar that a later reindex applies, while the thumbnail
            // and DB stay on their previous state. A failed sidecar write
            // aborts the request before the thumbnail is touched.
            write_rotation_for(&abstract_data, next_rotation).map_err(|e| {
                AppError::new(
                    ErrorKind::IO,
                    format!("Failed to persist rotation to the XMP sidecar: {e}"),
                )
            })?;

            // Generate and save the rotated thumbnail
            generate_thumbnail_for_image(&mut abstract_data, &dyn_img.clone()).or_raise(|| {
                (
                    ErrorKind::Internal,
                    "Failed to generate thumbnail for rotated image",
                )
            })?;

            // Update thumbhash and phash with the rotated image
            abstract_data.set_thumbhash(generate_thumbhash(&dyn_img));
            abstract_data.set_phash(generate_phash(&dyn_img));
            abstract_data.update_update_at();

            let album_ids: Vec<_> = abstract_data.album().into_iter().collect();

            let mut result_vec = vec![abstract_data];

            for album_id in album_ids {
                if let Ok(Some(album)) = asset_store::lookup_abstract_data_by_asset_id(&album_id) {
                    let mut album = album;
                    album.update_update_at();
                    result_vec.push(album);
                }
            }

            Ok(result_vec)
        })
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to spawn blocking task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(FlushTreeTask::insert(abstract_data))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to execute FlushTreeTask"))?;

    info!("Image rotated successfully");
    Ok(())
}
