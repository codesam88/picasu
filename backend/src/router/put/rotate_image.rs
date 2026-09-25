use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::openapi_components::Unauthorized;
use crate::process::misc::{generate_phash, generate_thumbhash};
use crate::process::thumbnail::generate_thumbnail_for_image;
use crate::router::{AppResult, GuardResult};
use crate::storage::asset_store;
use crate::tasks::batcher::flush_tree::FlushTreeTask;

use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::tasks::INDEX_COORDINATOR;
use anyhow::Result;
// use anyhow::anyhow;
use arrayvec::ArrayString;
use log::info;
use rocket::serde::{Deserialize, Serialize, json::Json};

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "rocket::serde")]
#[derive(utoipa::ToSchema)]
pub struct RotateImageRequest {
    /// Asset ID of the image to rotate
    pub asset_id: String,
}

#[utoipa::path(
        put,
        path = "/put/rotate-image",
        request_body = RotateImageRequest,
        responses(
            (status = 200, description = "Image rotated"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
        )
    )
]
#[put("/put/rotate-image", data = "<request>")]
pub async fn rotate_image(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    request: Json<RotateImageRequest>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;

    let _asset_id = ArrayString::<64>::from(&request.asset_id)
        .map_err(|_| AppError::new(ErrorKind::InvalidInput, "Invalid asset_id length or format"))?;

    let abstract_data =
        tokio::task::spawn_blocking(move || -> Result<Vec<AbstractData>, AppError> {
            let abstract_data = asset_store::lookup_abstract_data_by_asset_id(&request.asset_id)
                .or_raise(|| (ErrorKind::Database, "Failed to fetch DB record"))?
                .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Asset not found"))?;

            let mut abstract_data = abstract_data;

            // Only rotate images, not videos or albums
            if !matches!(abstract_data, AbstractData::Image(_)) {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    "Only images can be rotated",
                ));
            }

            // Load the compressed image (not the original)
            let compressed_path = abstract_data.compressed_path();
            let mut dyn_img = image::open(&compressed_path).map_err(|e| {
                AppError::new(
                    ErrorKind::IO,
                    format!(
                        "Failed to load compressed image: {} ({e})",
                        compressed_path.display()
                    ),
                )
            })?;

            // Rotate counter-clockwise (270 degrees clockwise = 90 degrees counter-clockwise)
            dyn_img = dyn_img.rotate270();

            // Swap width and height after rotation
            abstract_data.swap_width_height();

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

    INDEX_COORDINATOR
        .execute_batch_waiting(FlushTreeTask::insert(abstract_data))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to execute FlushTreeTask"))?;

    info!("Image rotated successfully");
    Ok(())
}
