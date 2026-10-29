use log::error;
use rocket::put;
use rocket::serde::json::Json;
use std::path::{Component, Path};
use tokio::task::spawn_blocking;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::config::AppConfig;
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAdmin;
use crate::router::auth::GuardMutate;
use crate::router::get::admin_config::RuntimeConfigDocument;
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

/// Patch the runtime-tier configuration.
///
/// Applies the posted runtime fields, rewrites `config.toml` and returns the
/// applied subset. Host-only and managed knobs in the body are rejected.
/// Admin-only and read-only-gated.
///
/// Errors: 400 host-only or managed knob present, or `uploadFolder` is absolute
/// or contains `..` — 401 missing or invalid admin credentials — 405 read-only
/// mode — 500 config write failure.
#[utoipa::path(
    tag = "config",
    request_body = PartialUpdateConfigRequest,
    security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAdmin", "GuardMutate"],
            "credentials": ["session"],
            "admin": "admin",
            "shareScope": "n-a",
            "public": false,
            "effects": ["manage"],
            "invariants": ["AUTH-11", "AUTH-12"]
        }))),
    responses(
        (status = 200, description = "Config updated", body = AdminConfigUpdateResponse),
        (status = 400, description = "Invalid input"),
        (status = 422, description = "Unprocessable entity"),
        (status = 401, response = Unauthorized),
        (status = 405, description = "Read-only mode"),
        (status = 500, description = "Internal error"),
    )
)]
#[put("/put/admin/config", data = "<req>")]
pub async fn update_admin_config_handler(
    auth: GuardResult<GuardAdmin>,
    read_only: GuardResult<GuardMutate>,
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct PartialUpdateConfigRequest {
    /// Host-only (`PICASU_ADDRESS` / config `server.address`). Any presence
    /// in the API body is rejected with 400 and changes nothing.
    pub address: Option<String>,
    /// Host-only (`PICASU_PORT` / config `server.port`). Any presence
    /// in the API body is rejected with 400 and changes nothing.
    pub port: Option<u16>,
    /// `None` = don't touch; `Some("")` resets to the default ("uploads").
    pub upload_folder: Option<String>,
    /// `None` = don't touch; `Some("")` resets to the default ("100MiB").
    pub max_upload_size: Option<String>,
    pub read_only_mode: Option<bool>,
    pub fs_notify_watcher: Option<bool>,
    pub normalize_upload_filenames: Option<bool>,
    pub validate_upload_content: Option<bool>,
    pub use_client_timestamp_info: Option<bool>,
    /// Runtime: `None` = don't touch.
    pub allow_registration: Option<bool>,
    /// Host-only (`--trivial-passwords` / `PICASU_TRIVIAL_PASSWORDS` /
    /// config `gallery.trivial_passwords`). Any presence in the API body is
    /// rejected with 400 and changes nothing.
    pub trivial_passwords: Option<bool>,
    /// Host-only (`--bootstrap-admin` / `PICASU_BOOTSTRAP_ADMIN` / config
    /// `gallery.bootstrap_admin`). Any presence in the API body is rejected
    /// with 400 and changes nothing.
    pub bootstrap_admin: Option<bool>,
    /// Host-only (`PICASU_TRUST_PROXY_FORWARDED_FOR` / config
    /// `server.trust_proxy_forwarded_for`). Any presence in the API body is
    /// rejected with 400 and changes nothing.
    pub trust_proxy_forwarded_for: Option<bool>,
    /// Host-only login-throttle knob (`PICASU_LOGIN_IP_WINDOW_SECS` /
    /// config `server.login_ip_window_secs`). Any presence in the API body
    /// is rejected with 400 and changes nothing.
    pub login_ip_window_secs: Option<u64>,
    /// Host-only login-throttle knob (`PICASU_LOGIN_IP_MAX_FAILURES` /
    /// config `server.login_ip_max_failures`). Any presence in the API body
    /// is rejected with 400 and changes nothing.
    pub login_ip_max_failures: Option<u32>,
    /// Host-only login-throttle knob (`PICASU_LOGIN_IP_BASE_BAN_SECS` /
    /// config `server.login_ip_base_ban_secs`). Any presence in the API body
    /// is rejected with 400 and changes nothing.
    pub login_ip_base_ban_secs: Option<u64>,
    /// Host-only login-throttle knob (`PICASU_LOGIN_IP_MAX_BAN_SECS` /
    /// config `server.login_ip_max_ban_secs`). Any presence in the API body
    /// is rejected with 400 and changes nothing.
    pub login_ip_max_ban_secs: Option<u64>,
    /// Host-only login-throttle knob
    /// (`PICASU_LOGIN_IP_LONG_WINDOW_SECS` / config
    /// `server.login_ip_long_window_secs`). Any presence in the API body is
    /// rejected with 400 and changes nothing.
    pub login_ip_long_window_secs: Option<u64>,
    /// Host-only login-throttle knob
    /// (`PICASU_LOGIN_IP_LONG_MAX_FAILURES` / config
    /// `server.login_ip_long_max_failures`). Any presence in the API body
    /// is rejected with 400 and changes nothing.
    pub login_ip_long_max_failures: Option<u32>,
    /// Host-only login-throttle knob
    /// (`PICASU_LOGIN_IP_LONG_BASE_BAN_SECS` / config
    /// `server.login_ip_long_base_ban_secs`). Any presence in the API body
    /// is rejected with 400 and changes nothing.
    pub login_ip_long_base_ban_secs: Option<u64>,
    /// Host-only login-throttle knob
    /// (`PICASU_LOGIN_IP_LONG_MAX_BAN_SECS` / config
    /// `server.login_ip_long_max_ban_secs`). Any presence in the API body is
    /// rejected with 400 and changes nothing.
    pub login_ip_long_max_ban_secs: Option<u64>,
    /// Host-only login-throttle knob
    /// (`PICASU_LOGIN_USER_LONG_WINDOW_SECS` / config
    /// `server.login_user_long_window_secs`). Any presence in the API body
    /// is rejected with 400 and changes nothing.
    pub login_user_long_window_secs: Option<u64>,
    /// Host-only login-throttle knob
    /// (`PICASU_LOGIN_USER_LONG_MAX_FAILURES` / config
    /// `server.login_user_long_max_failures`). Any presence in the API body
    /// is rejected with 400 and changes nothing.
    pub login_user_long_max_failures: Option<u32>,
    /// Host-only login-throttle knob
    /// (`PICASU_LOGIN_USER_LONG_BASE_BAN_SECS` / config
    /// `server.login_user_long_base_ban_secs`). Any presence in the API body
    /// is rejected with 400 and changes nothing.
    pub login_user_long_base_ban_secs: Option<u64>,
    /// Host-only login-throttle knob
    /// (`PICASU_LOGIN_USER_LONG_MAX_BAN_SECS` / config
    /// `server.login_user_long_max_ban_secs`). Any presence in the API body
    /// is rejected with 400 and changes nothing.
    pub login_user_long_max_ban_secs: Option<u64>,
    /// Managed, never set: the signing key lives in the owner-only secrets
    /// file. Any presence in the API body is rejected with 400; rotate it
    /// with `POST /post/config/auth-key/reset`.
    pub auth_key: Option<String>,
}

pub(crate) fn host_only_knobs(
    req: &PartialUpdateConfigRequest,
) -> [(&'static str, bool, &'static str); 18] {
    [
        (
            "address",
            req.address.is_some(),
            "PICASU_ADDRESS or config `server.address`",
        ),
        (
            "port",
            req.port.is_some(),
            "PICASU_PORT or config `server.port`",
        ),
        (
            "trivialPasswords",
            req.trivial_passwords.is_some(),
            "--trivial-passwords, PICASU_TRIVIAL_PASSWORDS, or config `gallery.trivial_passwords`",
        ),
        (
            "bootstrapAdmin",
            req.bootstrap_admin.is_some(),
            "--bootstrap-admin, PICASU_BOOTSTRAP_ADMIN, or config `gallery.bootstrap_admin`",
        ),
        (
            "trustProxyForwardedFor",
            req.trust_proxy_forwarded_for.is_some(),
            "PICASU_TRUST_PROXY_FORWARDED_FOR or config `server.trust_proxy_forwarded_for`",
        ),
        (
            "loginIpWindowSecs",
            req.login_ip_window_secs.is_some(),
            "PICASU_LOGIN_IP_WINDOW_SECS or config `server.login_ip_window_secs`",
        ),
        (
            "loginIpMaxFailures",
            req.login_ip_max_failures.is_some(),
            "PICASU_LOGIN_IP_MAX_FAILURES or config `server.login_ip_max_failures`",
        ),
        (
            "loginIpBaseBanSecs",
            req.login_ip_base_ban_secs.is_some(),
            "PICASU_LOGIN_IP_BASE_BAN_SECS or config `server.login_ip_base_ban_secs`",
        ),
        (
            "loginIpMaxBanSecs",
            req.login_ip_max_ban_secs.is_some(),
            "PICASU_LOGIN_IP_MAX_BAN_SECS or config `server.login_ip_max_ban_secs`",
        ),
        (
            "loginIpLongWindowSecs",
            req.login_ip_long_window_secs.is_some(),
            "PICASU_LOGIN_IP_LONG_WINDOW_SECS or config `server.login_ip_long_window_secs`",
        ),
        (
            "loginIpLongMaxFailures",
            req.login_ip_long_max_failures.is_some(),
            "PICASU_LOGIN_IP_LONG_MAX_FAILURES or config `server.login_ip_long_max_failures`",
        ),
        (
            "loginIpLongBaseBanSecs",
            req.login_ip_long_base_ban_secs.is_some(),
            "PICASU_LOGIN_IP_LONG_BASE_BAN_SECS or config `server.login_ip_long_base_ban_secs`",
        ),
        (
            "loginIpLongMaxBanSecs",
            req.login_ip_long_max_ban_secs.is_some(),
            "PICASU_LOGIN_IP_LONG_MAX_BAN_SECS or config `server.login_ip_long_max_ban_secs`",
        ),
        (
            "loginUserLongWindowSecs",
            req.login_user_long_window_secs.is_some(),
            "PICASU_LOGIN_USER_LONG_WINDOW_SECS or config `server.login_user_long_window_secs`",
        ),
        (
            "loginUserLongMaxFailures",
            req.login_user_long_max_failures.is_some(),
            "PICASU_LOGIN_USER_LONG_MAX_FAILURES or config `server.login_user_long_max_failures`",
        ),
        (
            "loginUserLongBaseBanSecs",
            req.login_user_long_base_ban_secs.is_some(),
            "PICASU_LOGIN_USER_LONG_BASE_BAN_SECS or config `server.login_user_long_base_ban_secs`",
        ),
        (
            "loginUserLongMaxBanSecs",
            req.login_user_long_max_ban_secs.is_some(),
            "PICASU_LOGIN_USER_LONG_MAX_BAN_SECS or config `server.login_user_long_max_ban_secs`",
        ),
        (
            "authKey",
            req.auth_key.is_some(),
            "rotate it with `POST /post/config/auth-key/reset`",
        ),
    ]
}

/// Patch the server configuration with the posted fields.
///
/// Every field is optional and a field absent from the body keeps its current
/// value. `address`, `port`, `trivialPasswords`,
/// `bootstrapAdmin`, and the login-throttle knobs
/// (`trustProxyForwardedFor`, `login*Secs`, `login*MaxFailures`) are
/// host-only (env / config file, restart-required) and `authKey`
/// is managed (secrets file + reset endpoint); any presence of these in the
/// body is a 400. The only other validation is on
/// `uploadFolder`, which must be a relative path free of `..` components;
/// on success `config.toml` is rewritten, the in-memory configuration is
/// replaced and the filesystem watcher is reloaded.
///
/// Corner cases: `uploadFolder` and `maxUploadSize` take an empty string
/// to reset to their defaults, `uploads` and `100MiB`.
///
/// Errors: 400 `uploadFolder` is absolute or contains `..`, the body cannot
/// be parsed, or a host-only or managed knob is present — 401 missing or invalid admin
/// credentials; share tokens are not accepted — 405 read-only mode — 500
/// config write failure.
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
            .body(serde_json::json!({ "readOnlyMode": false }).to_string())
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
            .body(serde_json::json!({ "readOnlyMode": false }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Unauthorized);
    }
    #[test]
    fn put_admin_config_rejects_auth_key_as_managed() {
        let _g = lock();
        let client = setup();
        seed("pac-authkey", true, None);
        let status = client
            .put("/put/admin/config")
            .cookie(cookie("pac-authkey", true))
            .header(ContentType::JSON)
            .body(serde_json::json!({ "authKey": "smuggled-key" }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::BadRequest);
    }
    #[test]
    fn put_admin_config_rejects_invalid_upload_folder() {
        let _g = lock();
        let client = setup();
        seed("pac-folder", true, None);
        let before = crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .read()
            .expect("lock")
            .upload_folder
            .clone();
        for body in [
            serde_json::json!({ "uploadFolder": "/abs/path" }),
            serde_json::json!({ "uploadFolder": "../escape" }),
        ] {
            let status = client
                .put("/put/admin/config")
                .cookie(cookie("pac-folder", true))
                .header(ContentType::JSON)
                .body(body.to_string())
                .dispatch()
                .status();
            assert_eq!(status, Status::BadRequest, "invalid folder, body {body}");
        }
        let after = crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .read()
            .expect("lock")
            .upload_folder
            .clone();
        assert_eq!(before, after, "rejected folder must change nothing");
    }

    #[test]
    fn put_admin_config_rejects_host_only_knobs() {
        let _g = lock();
        let client = setup();
        seed("pac-host", true, None);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .bootstrap_admin = false;
        let snapshot = || {
            let cfg = crate::model::config::APP_CONFIG
                .get()
                .expect("APP_CONFIG")
                .read()
                .expect("lock");
            (
                cfg.bootstrap_admin,
                cfg.login_ip_window_secs,
                cfg.trivial_passwords,
            )
        };
        let before = snapshot();
        for body in [
            serde_json::json!({ "bootstrapAdmin": true }),
            serde_json::json!({ "loginIpWindowSecs": 100 }),
            serde_json::json!({ "trivialPasswords": true }),
        ] {
            let status = client
                .put("/put/admin/config")
                .cookie(cookie("pac-host", true))
                .header(ContentType::JSON)
                .body(body.to_string())
                .dispatch()
                .status();
            assert_eq!(status, Status::BadRequest, "host-only knob, body {body}");
        }
        assert_eq!(snapshot(), before, "rejected knobs must change nothing");
    }
}
