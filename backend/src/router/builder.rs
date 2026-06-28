use super::auth::generate_fairing_routes;
use super::cache::cache_control_fairing;
use super::delete::generate_delete_routes;
use super::get::generate_get_routes;
use super::post::generate_post_routes;
use super::put::generate_put_routes;
use crate::model::config::{APP_CONFIG, AppConfig};
use rocket::data::{ByteUnit, Limits};
#[cfg(not(feature = "embed-frontend"))]
use rocket::fs::FileServer;
use rocket::info;
use std::path::PathBuf;

#[cfg(test)]
fn create_dummy_config() -> AppConfig {
    let mut config = AppConfig::default();
    config.address = "127.0.0.1".to_string();
    config.port = 8000;
    config
}

/// Handle routes for embedded frontend assets
#[cfg(feature = "embed-frontend")]
#[get("/assets/<file..>")]
async fn assets(
    file: PathBuf,
) -> Option<(rocket::http::ContentType, std::borrow::Cow<'static, [u8]>)> {
    use crate::frontend::FrontendAssets;

    let filename = format!("assets/{}", file.display());
    let asset = FrontendAssets::get(&filename)?;

    let mime = mime_guess::from_path(&filename).first_or_octet_stream();
    let content_type = rocket::http::ContentType::parse_flexible(mime.as_ref())
        .unwrap_or(rocket::http::ContentType::Binary);

    Some((content_type, asset.data))
}

/// Load configuration from the already-initialized global config.
pub fn load_config() -> AppConfig {
    APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned")
        .clone()
}

/// Build and configure Rocket instance
pub fn build_rocket() -> rocket::Rocket<rocket::Build> {
    let app_config = load_config();
    build_rocket_with_config(app_config)
}

/// Build Rocket instance with injected configuration
///
/// # Panics
/// Panics if `app_config.max_upload_size` is not a valid [`ByteUnit`] string.
#[must_use]
pub fn build_rocket_with_config(mut app_config: AppConfig) -> rocket::Rocket<rocket::Build> {
    if let Ok(port_str) = std::env::var("PICASU_PORT")
        && let Ok(port) = port_str.parse::<u16>()
    {
        app_config.port = port;
    }
    if let Ok(addr) = std::env::var("PICASU_ADDRESS") {
        app_config.address = addr;
    }

    let max_upload = app_config
        .max_upload_size
        .parse::<ByteUnit>()
        .unwrap_or_else(|_| panic!("Invalid max_upload_size: '{}'", app_config.max_upload_size));

    let limits = Limits::default()
        .limit("file", max_upload)
        .limit("data-form", max_upload);

    let rocket_config = rocket::Config::figment()
        .merge(("address", &app_config.address))
        .merge(("port", app_config.port))
        .merge(("limits", limits));

    let base_app = rocket::custom(rocket_config)
        .manage(app_config)
        .attach(cache_control_fairing());

    let app = mount_frontend(base_app);

    app.mount("/", generate_get_routes())
        .mount("/", generate_post_routes())
        .mount("/", generate_put_routes())
        .mount("/", generate_delete_routes())
        .mount("/", generate_fairing_routes())
}

#[cfg(test)]
mod test_build_rocket_with_config {
    use super::*;

    #[test]
    fn test_build_rocket_config() {
        let mut config = create_dummy_config();
        config.port = 9999;
        config.address = "0.0.0.0".to_string();
        config.max_upload_size = "500MiB".to_string();

        let rocket = build_rocket_with_config(config);
        let figment = rocket.figment();

        let port: u16 = figment.extract_inner("port").expect("port not found");
        let address: String = figment.extract_inner("address").expect("address not found");

        assert_eq!(port, 9999);
        assert_eq!(address, "0.0.0.0");

        let limits: rocket::data::Limits =
            figment.extract_inner("limits").expect("limits not found");
        assert_eq!(limits.get("file"), Some(ByteUnit::MiB * 500));
        assert_eq!(limits.get("data-form"), Some(ByteUnit::MiB * 500));
    }
}

fn mount_frontend(app: rocket::Rocket<rocket::Build>) -> rocket::Rocket<rocket::Build> {
    #[cfg(feature = "embed-frontend")]
    {
        info!("Serving assets from embedded binary");
        app.mount("/", routes![assets])
    }

    #[cfg(not(feature = "embed-frontend"))]
    {
        let asset_path = PathBuf::from("../frontend/dist/assets");
        info!("Serving assets from: {:?}", asset_path);
        app.mount("/assets", FileServer::from(asset_path))
    }
}
