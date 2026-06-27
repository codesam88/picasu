// src/router/post/import_config.rs

use log::error;
use rocket::http::Status;
use rocket::post;
use rocket::serde::json::Json;

use crate::error::{AppError, ErrorKind};
use crate::model::config::AppConfig;
use crate::router::AppResult;
use crate::router::auth::GuardAuth;

#[utoipa::path(
        post,
        path = "/post/config/import",
        request_body = AppConfig,
        responses(
            (status = 200, description = "Config imported"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[post("/post/config/import", data = "<file>")]
pub fn import_config_handler(_auth: GuardAuth, file: Json<AppConfig>) -> AppResult<Status> {
    match AppConfig::update(file.into_inner()) {
        Ok(()) => Ok(Status::Ok),
        Err(e) => {
            error!("Import failed: {e}");
            Err(AppError::from_err(ErrorKind::Internal, e))
        }
    }
}
