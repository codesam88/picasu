use rocket::get;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::model::config::config_tier_table;
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAdmin;
use crate::router::{AppResult, GuardResult};

/// One annotated configuration field as returned by `GET /get/admin/config`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
#[allow(clippy::struct_excessive_bools)]
pub struct ConfigFieldEntry {
    pub name: String,
    /// The live value: scalars for every tier — the schema must not pin a type.
    #[schema(value_type = serde_json::Value)]
    pub value: serde_json::Value,
    pub tier: String,
    pub source: String,
    pub requires_restart: bool,
    pub writable: bool,
}

/// The runtime-tier subset of the configuration, shared by the admin PUT
/// response and the versioned export envelope.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
#[allow(clippy::struct_excessive_bools)]
pub struct RuntimeConfigDocument {
    pub upload_folder: String,
    pub max_upload_size: String,
    pub read_only_mode: bool,
    pub fs_notify_watcher: bool,
    pub normalize_upload_filenames: bool,
    pub validate_upload_content: bool,
    pub use_client_timestamp_info: bool,
    pub allow_registration: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct AdminConfigExportResponse {
    pub version: u32,
    pub scope: String,
    pub config: RuntimeConfigDocument,
}

fn field_value(config: &crate::model::config::AppConfig, name: &str) -> serde_json::Value {
    match name {
        "address" => serde_json::to_value(&config.address).unwrap_or_default(),
        "port" => serde_json::to_value(config.port).unwrap_or_default(),
        "data_home" => serde_json::to_value(&config.data_home).unwrap_or_default(),
        "image_home" => serde_json::to_value(&config.image_home).unwrap_or_default(),
        "users_home" => serde_json::to_value(&config.users_home).unwrap_or_default(),
        "upload_folder" => serde_json::to_value(&config.upload_folder).unwrap_or_default(),
        "max_upload_size" => serde_json::to_value(&config.max_upload_size).unwrap_or_default(),
        "read_only_mode" => serde_json::to_value(config.read_only_mode).unwrap_or_default(),
        "fs_notify_watcher" => serde_json::to_value(config.fs_notify_watcher).unwrap_or_default(),
        "normalize_upload_filenames" => {
            serde_json::to_value(config.normalize_upload_filenames).unwrap_or_default()
        }
        "validate_upload_content" => {
            serde_json::to_value(config.validate_upload_content).unwrap_or_default()
        }
        "use_client_timestamp_info" => {
            serde_json::to_value(config.use_client_timestamp_info).unwrap_or_default()
        }
        "allow_registration" => serde_json::to_value(config.allow_registration).unwrap_or_default(),
        "trivial_passwords" => serde_json::to_value(config.trivial_passwords).unwrap_or_default(),
        "bootstrap_admin" => serde_json::to_value(config.bootstrap_admin).unwrap_or_default(),
        "trust_proxy_forwarded_for" => {
            serde_json::to_value(config.trust_proxy_forwarded_for).unwrap_or_default()
        }
        "login_ip_window_secs" => {
            serde_json::to_value(config.login_ip_window_secs).unwrap_or_default()
        }
        "login_ip_max_failures" => {
            serde_json::to_value(config.login_ip_max_failures).unwrap_or_default()
        }
        "login_ip_base_ban_secs" => {
            serde_json::to_value(config.login_ip_base_ban_secs).unwrap_or_default()
        }
        "login_ip_max_ban_secs" => {
            serde_json::to_value(config.login_ip_max_ban_secs).unwrap_or_default()
        }
        "login_ip_long_window_secs" => {
            serde_json::to_value(config.login_ip_long_window_secs).unwrap_or_default()
        }
        "login_ip_long_max_failures" => {
            serde_json::to_value(config.login_ip_long_max_failures).unwrap_or_default()
        }
        "login_ip_long_base_ban_secs" => {
            serde_json::to_value(config.login_ip_long_base_ban_secs).unwrap_or_default()
        }
        "login_ip_long_max_ban_secs" => {
            serde_json::to_value(config.login_ip_long_max_ban_secs).unwrap_or_default()
        }
        "login_user_long_window_secs" => {
            serde_json::to_value(config.login_user_long_window_secs).unwrap_or_default()
        }
        "login_user_long_max_failures" => {
            serde_json::to_value(config.login_user_long_max_failures).unwrap_or_default()
        }
        "login_user_long_base_ban_secs" => {
            serde_json::to_value(config.login_user_long_base_ban_secs).unwrap_or_default()
        }
        "login_user_long_max_ban_secs" => {
            serde_json::to_value(config.login_user_long_max_ban_secs).unwrap_or_default()
        }
        "auth_key" => serde_json::to_value(config.auth_key.is_some()).unwrap_or_default(),
        "web_root" => serde_json::to_value(&config.web_root).unwrap_or_default(),
        other => {
            debug_assert!(false, "unhandled config field: {other}");
            serde_json::Value::Null
        }
    }
}

/// Decide whether `field` reports `env` or `file` as its source, mirroring
/// `AppConfig::apply_env_overrides_with`: an env var that is missing, empty,
/// whitespace-only, or unparseable for a typed field counts as `file`.
fn source_for(field_name: &str, env_var: &str) -> &'static str {
    let Ok(raw) = std::env::var(env_var) else {
        return "file";
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "file";
    }
    match field_name {
        "port" => {
            if trimmed.parse::<u16>().is_ok() {
                "env"
            } else {
                "file"
            }
        }
        "login_ip_max_failures" | "login_ip_long_max_failures" | "login_user_long_max_failures" => {
            if trimmed.parse::<u32>().is_ok() {
                "env"
            } else {
                "file"
            }
        }
        name if name.starts_with("login_") && name.ends_with("_secs") => {
            if trimmed.parse::<u64>().is_ok() {
                "env"
            } else {
                "file"
            }
        }
        "read_only_mode"
        | "fs_notify_watcher"
        | "allow_registration"
        | "trivial_passwords"
        | "bootstrap_admin"
        | "trust_proxy_forwarded_for" => {
            if trimmed == "true" || trimmed == "false" {
                "env"
            } else {
                "file"
            }
        }
        _ => "env",
    }
}

/// Full annotated view of every configuration field.
///
/// Returns one entry per known field with its current value, tier
/// (`host` vs `runtime`), source (`env` vs `file`) and whether it is
/// writable via the admin API. Admin-only; reading is always allowed
/// (no read-only guard).
///
/// Errors: 401 missing or invalid admin credentials.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAdmin"],
            "credentials": ["session"],
            "admin": "admin",
            "shareScope": "n-a",
            "effects": ["read"],
            "invariants": ["AUTH-11"]
        }))),
    responses(
        (status = 200, description = "Annotated configuration", body = Vec<ConfigFieldEntry>),
        (status = 401, response = Unauthorized),
    )
)]
#[get("/get/admin/config")]
pub fn get_admin_config_handler(
    auth: GuardResult<GuardAdmin>,
) -> AppResult<Json<Vec<ConfigFieldEntry>>> {
    let _ = auth?;
    let entries = crate::model::config::with_config(|config| {
        let mut entries = Vec::new();
        for field in config_tier_table::CONFIG_FIELDS {
            let tier_str = match field.tier {
                config_tier_table::ConfigTier::Host => "host",
                config_tier_table::ConfigTier::Runtime => "runtime",
            };
            let source = match field.env_var {
                Some(var) => source_for(field.name, var),
                None => "file",
            };
            let requires_restart = matches!(field.tier, config_tier_table::ConfigTier::Host);
            let writable = matches!(field.tier, config_tier_table::ConfigTier::Runtime);
            entries.push(ConfigFieldEntry {
                name: field.name.to_string(),
                value: field_value(config, field.name),
                tier: tier_str.to_string(),
                source: source.to_string(),
                requires_restart,
                writable,
            });
        }
        entries
    });
    Ok(Json(entries))
}

/// Export the runtime-tier configuration as a versioned envelope.
///
/// Returns the writable runtime subset with a version and scope marker so
/// backups stay safe to store alongside the data. The signing key is never
/// exported. Admin-only.
///
/// Errors: 401 missing or invalid admin credentials.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAdmin"],
            "credentials": ["session"],
            "admin": "admin",
            "shareScope": "n-a",
            "effects": ["read"],
            "invariants": ["AUTH-11"]
        }))),
    responses(
        (status = 200, description = "Exported runtime configuration", body = AdminConfigExportResponse),
        (status = 401, response = Unauthorized),
    )
)]
#[get("/get/admin/config/export")]
pub fn get_admin_config_export_handler(
    auth: GuardResult<GuardAdmin>,
) -> AppResult<Json<AdminConfigExportResponse>> {
    let _ = auth?;
    let config = crate::model::config::with_config(|c| RuntimeConfigDocument {
        upload_folder: c.upload_folder.clone(),
        max_upload_size: c.max_upload_size.clone(),
        read_only_mode: c.read_only_mode,
        fs_notify_watcher: c.fs_notify_watcher,
        normalize_upload_filenames: c.normalize_upload_filenames,
        validate_upload_content: c.validate_upload_content,
        use_client_timestamp_info: c.use_client_timestamp_info,
        allow_registration: c.allow_registration,
    });
    Ok(Json(AdminConfigExportResponse {
        version: 1,
        scope: "server-runtime".to_string(),
        config,
    }))
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Status};

    use crate::tests::bootstrap::{cookie, lock, seed, setup};

    #[test]
    fn get_admin_config_fields_all_mapped() {
        use crate::model::config::AppConfig;
        use std::path::PathBuf;
        let config = AppConfig {
            data_home: Some(PathBuf::from("/tmp/data")),
            image_home: Some(PathBuf::from("/tmp/images")),
            users_home: Some(PathBuf::from("/tmp/users")),
            web_root: Some(PathBuf::from("/tmp/web")),
            auth_key: Some("key".to_string()),
            ..AppConfig::default()
        };
        for field in crate::model::config::config_tier_table::CONFIG_FIELDS {
            let value = super::field_value(&config, field.name);
            assert_ne!(
                value,
                serde_json::Value::Null,
                "CONFIG_FIELDS entry `{}` must map to a non-Null value",
                field.name
            );
        }
    }

    #[test]
    fn get_admin_config_non_admin_401() {
        let _g = lock();
        let client = setup();
        seed("gac-nonadmin", false, None);
        let resp = client
            .get("/get/admin/config")
            .cookie(cookie("gac-nonadmin", false))
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
    }

    #[test]
    fn get_admin_config_requires_admin() {
        let _g = lock();
        let client = setup();
        seed("gac-root", true, None);
        let unauth = client.get("/get/admin/config").dispatch();
        assert_eq!(unauth.status(), Status::Unauthorized);
        let admin = client
            .get("/get/admin/config")
            .cookie(cookie("gac-root", true))
            .dispatch();
        assert_eq!(admin.status(), Status::Ok);
    }

    #[test]
    fn get_admin_config_works_while_read_only() {
        let _g = lock();
        let client = setup();
        seed("gac-ro", true, None);
        // flip read-only on in-memory
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = true;
        let resp = client
            .get("/get/admin/config")
            .cookie(cookie("gac-ro", true))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = false;
    }

    #[test]
    fn get_admin_config_export_redacts_auth_key() {
        let _g = lock();
        let client = setup();
        seed("gac-exp", true, None);
        let admin = cookie("gac-exp", true);
        let _ = client
            .post("/post/config/auth-key/reset")
            .cookie(admin.clone())
            .header(ContentType::JSON)
            .dispatch();
        let admin = cookie("gac-exp", true);
        let resp = client
            .get("/get/admin/config/export")
            .cookie(admin)
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("body");
        assert!(
            !body.contains("authKey"),
            "export must not name auth key: {body}"
        );
        assert!(body.contains("server-runtime"), "scope missing: {body}");
    }
    #[test]
    fn get_admin_config_export_works_while_read_only() {
        let _g = lock();
        let client = setup();
        seed("gac-exp-ro", true, None);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = true;
        let admin = cookie("gac-exp-ro", true);
        let resp = client
            .get("/get/admin/config/export")
            .cookie(admin)
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = false;
    }
}
