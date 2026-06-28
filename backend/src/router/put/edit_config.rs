use log::error;
use rocket::http::Status;
use rocket::put;
use rocket::serde::json::Json;
use tokio::task::spawn_blocking;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::config::{APP_CONFIG, AppConfig};
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct PartialUpdateConfigRequest {
    pub address: Option<String>,
    pub port: Option<u16>,
    /// `None` = don't touch; `Some("")` resets to the default ("uploads").
    pub upload_folder: Option<String>,
    /// `None` = don't touch; `Some("")` resets to the default ("100MiB").
    pub max_upload_size: Option<String>,
    pub read_only_mode: Option<bool>,
    pub disable_img: Option<bool>,
    pub auth_key: Option<String>,
}

#[utoipa::path(
        put,
        path = "/put/config",
        request_body = PartialUpdateConfigRequest,
        responses(
            (status = 200, description = "Config updated"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[put("/put/config", data = "<req>")]
pub async fn update_config_handler(
    _auth: GuardAuth,
    read_only: GuardResult<GuardReadOnlyMode>,
    req: Json<PartialUpdateConfigRequest>,
) -> AppResult<Status> {
    let _ = read_only?;
    let req_data = req.into_inner();

    spawn_blocking(move || -> Result<Status, AppError> {
        let mut current_config = {
            let read_lock = APP_CONFIG
                .get()
                .expect("APP_CONFIG not initialized")
                .read()
                .expect("lock poisoned");
            read_lock.clone()
        };

        if let Some(address) = req_data.address {
            current_config.address = address;
        }
        if let Some(port) = req_data.port {
            current_config.port = port;
        }
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
        if let Some(key) = req_data.auth_key {
            let trimmed = key.trim();
            current_config.auth_key = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        AppConfig::update(current_config).map_err(|e| {
            error!("Failed to update config: {e}");
            AppError::from_err(ErrorKind::Internal, e)
        })?;

        Ok(Status::Ok)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Task join error"))?
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct UpdatePasswordRequest {
    pub password: Option<String>,
    pub old_password: Option<String>,
}

#[utoipa::path(
        put,
        path = "/put/config/password",
        request_body = UpdatePasswordRequest,
        responses(
            (status = 200, description = "Password updated"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[put("/put/config/password", data = "<req>")]
pub async fn update_password_handler(
    _auth: GuardAuth,
    read_only: GuardResult<GuardReadOnlyMode>,
    req: Json<UpdatePasswordRequest>,
) -> AppResult<Status> {
    let _ = read_only?;
    let req_data = req.into_inner();

    spawn_blocking(move || -> Result<Status, AppError> {
        let mut current_config = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned")
            .clone();

        if req_data.old_password != current_config.password {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "Incorrect current password",
            ));
        }

        if let Some(pwd) = req_data.password {
            let trimmed_pwd = pwd.trim().to_string();
            current_config.password = if trimmed_pwd.is_empty() {
                None
            } else {
                Some(trimmed_pwd)
            };
        } else {
            current_config.password = None;
        }

        AppConfig::update(current_config).map_err(|e| {
            error!("Failed to update config: {e}");
            AppError::from_err(ErrorKind::Internal, e)
        })?;

        Ok(Status::Ok)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Task join error"))?
}
