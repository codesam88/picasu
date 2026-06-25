use rocket::get;
use rocket::http::ContentType;
use rocket::serde::json::Json;

use crate::public::structure::config::APP_CONFIG;
use crate::router::fairing::guard_auth::GuardAuth;
use crate::router::fairing::guard_share::GuardShare;
use serde::Serialize;

use crate::router::{AppResult, GuardResult};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConfigResponse {
    pub address: String,
    pub port: u16,
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    #[serde(rename = "imagePath", alias = "imageHome")]
    pub image_home: Option<std::path::PathBuf>,
    pub upload_folder: String,
    pub max_upload_size: String,
    pub read_only_mode: bool,
    pub disable_img: bool,
    pub has_password: bool,
    pub has_auth_key: bool,
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(
        get,
        path = "/get/config",
        responses(
            (status = 200, description = "Public configuration", body = ConfigResponse),
            (status = 400, description = "Invalid input"),
        )
    )
)]
#[get("/get/config")]
pub fn get_config_handler(auth: GuardResult<GuardShare>) -> AppResult<Json<ConfigResponse>> {
    let _ = auth?;
    let config = APP_CONFIG.get().unwrap().read().unwrap();
    let response = ConfigResponse {
        address: config.address.clone(),
        port: config.port,
        image_home: config.image_home.clone(),
        upload_folder: config.upload_folder.clone(),
        max_upload_size: config.max_upload_size.clone(),
        read_only_mode: config.read_only_mode,
        disable_img: config.disable_img,
        has_password: config.password.is_some(),
        has_auth_key: config.auth_key.is_some(),
    };
    Ok(Json(response))
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(
        get,
        path = "/get/config/export",
        responses(
            (status = 200, description = "Exported configuration", body = String),
            (status = 400, description = "Invalid input"),
        )
    )
)]
#[get("/get/config/export")]
pub fn export_config_handler(auth: GuardResult<GuardAuth>) -> AppResult<(ContentType, String)> {
    let _ = auth?;
    let config = APP_CONFIG.get().unwrap().read().unwrap();
    let json = serde_json::to_string_pretty(&*config).unwrap_or_default();
    Ok((ContentType::JSON, json))
}
