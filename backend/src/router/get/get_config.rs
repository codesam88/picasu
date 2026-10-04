use rocket::get;
use rocket::http::ContentType;
use rocket::serde::json::Json;

use crate::model::config::APP_CONFIG;
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardShare;
use serde::Serialize;

use crate::router::{AppResult, GuardResult};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
#[allow(clippy::struct_excessive_bools)]
pub struct ConfigResponse {
    pub address: String,
    pub port: u16,
    #[schema(value_type = Option<String>)]
    #[serde(rename = "imagePath", alias = "imageHome")]
    pub image_home: Option<std::path::PathBuf>,
    pub upload_folder: String,
    pub max_upload_size: String,
    pub read_only_mode: bool,
    pub disable_img: bool,
    pub fs_notify_watcher: bool,
    pub normalize_upload_filenames: bool,
    pub validate_upload_content: bool,
    pub use_client_timestamp_info: bool,
    pub has_password: bool,
    pub has_auth_key: bool,
}

/// Serve the client-visible server configuration.
///
/// Returns the values a client needs to drive the other operations: address
/// and port, the image library root (`imagePath`), `uploadFolder`,
/// `maxUploadSize`, `readOnlyMode`, `disableImg`, `fsNotifyWatcher`,
/// `normalizeUploadFilenames`, `validateUploadContent`,
/// `useClientTimestampInfo`, plus `hasPassword` and `hasAuthKey`.
///
/// Corner cases: secrets are never returned — `hasPassword` and `hasAuthKey`
/// only report whether a password or an auth key is configured. `imagePath` is
/// the absolute library root and serializes as `null` when it is unset.
///
/// Errors: 400 half-supplied share credentials or an id that is not an album —
/// 401 no valid admin or share credentials.
#[utoipa::path(
        tag = "config",
        responses(
            (status = 200, description = "Public configuration", body = ConfigResponse),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
        )
    )
]
#[get("/get/config")]
pub fn get_config_handler(auth: GuardResult<GuardShare>) -> AppResult<Json<ConfigResponse>> {
    let _ = auth?;
    let config = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned");
    let response = ConfigResponse {
        address: config.address.clone(),
        port: config.port,
        image_home: config.image_home.clone(),
        upload_folder: config.upload_folder.clone(),
        max_upload_size: config.max_upload_size.clone(),
        read_only_mode: config.read_only_mode,
        disable_img: config.disable_img,
        fs_notify_watcher: config.fs_notify_watcher,
        normalize_upload_filenames: config.normalize_upload_filenames,
        validate_upload_content: config.validate_upload_content,
        use_client_timestamp_info: config.use_client_timestamp_info,
        has_password: config.password.is_some(),
        has_auth_key: config.auth_key.is_some(),
    };
    Ok(Json(response))
}

/// Export the full server configuration as JSON.
///
/// Returns the complete serialized configuration, including `password` and
/// `authKey` in plaintext whenever they are set, so the response has to be
/// handled as a secret. `webRoot` is the only field never serialized.
///
/// Errors: 400 invalid input — 401 no valid admin credentials.
#[utoipa::path(
        tag = "config",
        responses(
            (status = 200, description = "Exported configuration", body = String),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
        )
    )
]
#[get("/get/config/export")]
pub fn export_config_handler(auth: GuardResult<GuardAuth>) -> AppResult<(ContentType, String)> {
    let _ = auth?;
    let config = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned");
    let json = serde_json::to_string_pretty(&*config).unwrap_or_default();
    Ok((ContentType::JSON, json))
}
