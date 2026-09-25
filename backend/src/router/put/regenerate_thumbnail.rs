use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::openapi_components::Unauthorized;
use crate::process::misc::generate_dynamic_image;
use crate::process::misc::{generate_phash, generate_thumbhash};
use crate::router::{AppResult, GuardResult};
use crate::storage::asset_store;
use crate::tasks::batcher::flush_tree::FlushTreeTask;

use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::tasks::INDEX_COORDINATOR;
use anyhow::Result;
use arrayvec::ArrayString;
use log::info;
use rocket::form::{Errors, Form};
use rocket::fs::TempFile;

#[derive(FromForm, Debug)]
pub struct RegenerateThumbnailForm<'r> {
    /// Asset ID of the image to regenerate thumbnail for
    #[field(name = "asset_id")]
    pub asset_id: String,

    /// Frame file to use for thumbnail generation
    #[field(name = "frame")]
    pub frame: TempFile<'r>,
}

#[utoipa::path(
        put,
        path = "/put/regenerate-thumbnail-with-frame",
        tag = "assets",
        request_body = Value,
        responses(
            (status = 200, description = "Thumbnail regenerated"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
        )
    )
]
#[put("/put/regenerate-thumbnail-with-frame", data = "<form>")]
pub async fn regenerate_thumbnail_with_frame(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    form: Result<Form<RegenerateThumbnailForm<'_>>, Errors<'_>>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;
    let mut inner_form = match form {
        Ok(form) => form.into_inner(),
        Err(errors) => {
            let error_msg = errors
                .iter()
                .fold(String::from("Form parsing failed: "), |acc, e| {
                    format!("{acc}; {e}")
                });
            return Err(AppError::new(ErrorKind::InvalidInput, error_msg));
        }
    };

    // Convert asset_id string to ArrayString
    let asset_id = ArrayString::<64>::from(&inner_form.asset_id)
        .map_err(|_| AppError::new(ErrorKind::InvalidInput, "Invalid asset_id length or format"))?;

    let root = crate::storage::files::get_data_path();
    // Use asset_id for the compressed file path
    let file_path = root.join(format!(
        "object/compressed/{}/{}.jpg",
        &asset_id[0..2],
        asset_id.as_str()
    ));

    inner_form
        .frame
        .move_copy_to(&file_path)
        .await
        .or_raise(|| (ErrorKind::IO, "Failed to copy frame file"))?;

    let abstract_data = tokio::task::spawn_blocking(move || -> Result<AbstractData, AppError> {
        let abstract_data = asset_store::lookup_abstract_data_by_asset_id(&inner_form.asset_id)
            .or_raise(|| (ErrorKind::Database, "Failed to fetch DB record"))?
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Asset not found"))?;

        let mut abstract_data = abstract_data;

        let dyn_img = generate_dynamic_image(&abstract_data)
            .or_raise(|| (ErrorKind::Internal, "Failed to decode DynamicImage"))?;

        abstract_data.set_thumbhash(generate_thumbhash(&dyn_img));
        abstract_data.set_phash(generate_phash(&dyn_img));
        abstract_data.update_update_at();

        Ok(abstract_data)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to spawn blocking task"))??;

    INDEX_COORDINATOR
        .execute_batch_waiting(FlushTreeTask::insert(vec![abstract_data]))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to execute FlushTreeTask"))?;

    info!("Regenerating thumbnail successfully");
    Ok(())
}
