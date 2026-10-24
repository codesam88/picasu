use log::error;
use rocket::put;
use rocket::serde::json::Json;
use std::path::{Component, Path};
use tokio::task::spawn_blocking;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::config::AppConfig;
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::get::admin_config::RuntimeConfigDocument;
use crate::router::put::edit_config::PartialUpdateConfigRequest;
use crate::router::put::edit_config::host_only_knobs;
use crate::router::{AppResult, GuardResult};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
#[allow(clippy::struct_excessive_bools)]
pub struct AdminConfigUpdateResponse {
    pub config: RuntimeConfigDocument,
    #[serde(rename = "restartRequired")]
    pub restart_required: Vec<String>,
}

/// Patch the runtime-tier configuration. Admin-only and read-only-gated.
#[utoipa::path(
    tag = "config",
    request_body = PartialUpdateConfigRequest,
    responses(
        (status = 200, description = "Config updated", body = AdminConfigUpdateResponse),
        (status = 400, description = "Invalid input"),
        (status = 401, response = Unauthorized),
        (status = 405, description = "Read-only mode"),
        (status = 500, description = "Internal error"),
    )
)]
#[put("/put/admin/config", data = "<req>")]
pub async fn update_admin_config_handler(
    auth: GuardResult<GuardAuth>,
    read_only: GuardResult<GuardReadOnlyMode>,
    req: Json<PartialUpdateConfigRequest>,
) -> AppResult<Json<AdminConfigUpdateResponse>> {
    let _ = auth?;
    let _ = read_only?;
    let req_data = req.into_inner();

    spawn_blocking(
        move || -> Result<Json<AdminConfigUpdateResponse>, AppError> {
            if let Some((name, _, setters)) = host_only_knobs(&req_data)
                .into_iter()
                .find(|(_, present, _)| *present)
            {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    format!("{name} cannot be set via the API: {setters}"),
                ));
            }
            if let Some(ref folder) = req_data.upload_folder {
                let p = Path::new(folder.as_str());
                if p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir)) {
                    return Err(AppError::new(
                        ErrorKind::InvalidInput,
                        "upload_folder must be a relative path without '..' components",
                    ));
                }
            }

            let mut current_config =
                crate::model::config::with_config(crate::model::config::AppConfig::clone);

            if let Some(upload_folder) = req_data.upload_folder {
                current_config.upload_folder = upload_folder;
            }
            if let Some(max_upload_size) = req_data.max_upload_size {
                current_config.max_upload_size = max_upload_size;
            }
            if let Some(read_only_mode) = req_data.read_only_mode {
                current_config.read_only_mode = read_only_mode;
            }
            if let Some(disable_img) = req_data.disable_img {
                current_config.disable_img = disable_img;
            }
            if let Some(fs_notify_watcher) = req_data.fs_notify_watcher {
                current_config.fs_notify_watcher = fs_notify_watcher;
            }
            if let Some(normalize_upload_filenames) = req_data.normalize_upload_filenames {
                current_config.normalize_upload_filenames = normalize_upload_filenames;
            }
            if let Some(validate_upload_content) = req_data.validate_upload_content {
                current_config.validate_upload_content = validate_upload_content;
            }
            if let Some(use_client_timestamp_info) = req_data.use_client_timestamp_info {
                current_config.use_client_timestamp_info = use_client_timestamp_info;
            }
            if let Some(allow_registration) = req_data.allow_registration {
                current_config.allow_registration = allow_registration;
            }
            AppConfig::update(current_config).map_err(|e| {
                error!("Failed to update config: {e}");
                AppError::from_err(ErrorKind::Internal, e)
            })?;

            let applied = crate::model::config::with_config(|c| RuntimeConfigDocument {
                upload_folder: c.upload_folder.clone(),
                max_upload_size: c.max_upload_size.clone(),
                read_only_mode: c.read_only_mode,
                disable_img: c.disable_img,
                fs_notify_watcher: c.fs_notify_watcher,
                normalize_upload_filenames: c.normalize_upload_filenames,
                validate_upload_content: c.validate_upload_content,
                use_client_timestamp_info: c.use_client_timestamp_info,
                allow_registration: c.allow_registration,
            });
            Ok(Json(AdminConfigUpdateResponse {
                config: applied,
                restart_required: vec![],
            }))
        },
    )
    .await
    .or_raise(|| (ErrorKind::Internal, "Task join error"))?
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Status};

    use crate::tests::bootstrap::{cookie, lock, seed, setup};

    #[test]
    fn put_admin_config_rejects_host_field() {
        let _g = lock();
        let client = setup();
        seed("pac-root", true, None);
        let status = client
            .put("/put/admin/config")
            .cookie(cookie("pac-root", true))
            .header(ContentType::JSON)
            .body(serde_json::json!({ "address": "10.0.0.1" }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::BadRequest);
    }

    #[test]
    fn put_admin_config_accepts_runtime() {
        let _g = lock();
        let client = setup();
        seed("pac-root2", true, None);
        let status = client
            .put("/put/admin/config")
            .cookie(cookie("pac-root2", true))
            .header(ContentType::JSON)
            .body(serde_json::json!({ "allowRegistration": true }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn put_admin_config_read_only_405() {
        let _g = lock();
        let client = setup();
        seed("pac-ro", true, None);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = true;
        let status = client
            .put("/put/admin/config")
            .cookie(cookie("pac-ro", true))
            .header(ContentType::JSON)
            .body(serde_json::json!({ "readOnlyMode": false }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::MethodNotAllowed);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = false;
    }

    #[test]
    fn put_admin_config_non_admin_401() {
        let _g = lock();
        let client = setup();
        seed("pac-nonadmin", false, None);
        let status = client
            .put("/put/admin/config")
            .cookie(cookie("pac-nonadmin", false))
            .header(ContentType::JSON)
            .body(serde_json::json!({ "disableImg": false }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn put_admin_config_unauth_401() {
        let _g = lock();
        let client = setup();
        seed("pac-unauth", true, None);
        let status = client
            .put("/put/admin/config")
            .header(ContentType::JSON)
            .body(serde_json::json!({ "disableImg": false }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Unauthorized);
    }
}
