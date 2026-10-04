// src/router/post/import_config.rs

use log::error;
use rocket::http::Status;
use rocket::post;
use rocket::serde::json::Json;

use crate::error::{AppError, ErrorKind};
use crate::model::config::AppConfig;
use crate::openapi_components::Unauthorized;
use crate::router::AppResult;
use crate::router::auth::GuardAuth;

/// Replace the server configuration with the posted one.
///
/// The body is a complete `AppConfig` rather than a patch. Before writing,
/// `imagePath` is trimmed of whitespace and quotes (an empty value becomes
/// `null`), `uploadFolder` is trimmed, and an empty `authKey` is stored as
/// `null`, moving JWT signing onto the random secret generated once per
/// process. On success `config.toml` is rewritten, the in-memory configuration
/// is replaced and the filesystem watcher reloaded.
///
/// Corner cases: The posted configuration is accepted as-is: unlike
/// `PUT /put/config` it applies no validation, so an `uploadFolder` that is
/// absolute or contains `..` is stored here. A failed write is a 500 and leaves
/// the running configuration untouched.
///
/// Errors: 400 unusable request body — 401 missing or invalid credentials —
/// 500 storage failure.
#[utoipa::path(
        tag = "config",
        request_body = AppConfig,
        responses(
            (status = 200, description = "Config imported"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
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
