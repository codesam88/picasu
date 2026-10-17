use log::error;
use rocket::http::Status;
use rocket::put;
use rocket::serde::json::Json;
use std::path::{Component, Path};
use tokio::task::spawn_blocking;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::config::{APP_CONFIG, AppConfig};
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::auth::GuardUser;
use crate::router::put::users_password::{caller_identity, set_password_sync};
use crate::router::{AppResult, GuardResult};
use serde::{Deserialize, Serialize};

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
    pub disable_img: Option<bool>,
    pub fs_notify_watcher: Option<bool>,
    pub normalize_upload_filenames: Option<bool>,
    pub validate_upload_content: Option<bool>,
    pub use_client_timestamp_info: Option<bool>,
    /// Host-only (`PICASU_ALLOW_REGISTRATION` / config
    /// `gallery.allow_registration`). Any presence in the API body is
    /// rejected with 400 and changes nothing.
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

/// Host-only or managed knobs a client may never change at runtime via
/// this route: `(request field, is present, setter hint)`. The handler
/// rejects loudly (naming the setters) instead of persisting a value the
/// next boot would override or refuse. Rotation of the signing key goes
/// through `POST /post/config/auth-key/reset`.
fn host_only_knobs(req: &PartialUpdateConfigRequest) -> [(&'static str, bool, &'static str); 19] {
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
            "allowRegistration",
            req.allow_registration.is_some(),
            "PICASU_ALLOW_REGISTRATION or config `gallery.allow_registration`",
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
/// value. `address`, `port`, `allowRegistration`, `trivialPasswords`,
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
#[utoipa::path(
        tag = "config",
        request_body = PartialUpdateConfigRequest,
        responses(
            (status = 200, description = "Config updated"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
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

        let mut current_config = {
            let read_lock = APP_CONFIG
                .get()
                .expect("APP_CONFIG not initialized")
                .read()
                .expect("lock poisoned");
            read_lock.clone()
        };

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

/// Change the caller's own password.
///
/// This route changes the password of the authenticated caller only. A
/// self-change always requires the correct `oldPassword`, admins included.
/// The new `password` is trimmed and must satisfy
/// the default password policy after trimming (there is no "clear to open": open
/// mode is store-empty only). To change another user's password, use
/// `PUT /put/users/password`.
///
/// Corner cases: Any authenticated user reaches this route (share tokens
/// are rejected); unauthenticated callers are denied at dispatch. A missing
/// (`None`), blank, or too-short `password` is 400.
///
/// Errors: 400 missing, blank, or under-12 `password`, or the body cannot
/// be parsed — 401 missing or invalid credentials, or wrong `oldPassword`;
/// share tokens are not accepted — 405 read-only mode — 500 storage
/// failure.
#[utoipa::path(
        tag = "config",
        request_body = UpdatePasswordRequest,
        responses(
            (status = 200, description = "Password updated"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/config/password", data = "<req>")]
pub async fn update_password_handler(
    user: GuardUser,
    read_only: GuardResult<GuardReadOnlyMode>,
    req: Json<UpdatePasswordRequest>,
) -> AppResult<Status> {
    let _ = read_only?;
    let (caller_id, caller_admin) = caller_identity(&user.claims);
    let req_data = req.into_inner();
    spawn_blocking(move || -> Result<Status, AppError> {
        set_password_sync(
            &caller_id,
            caller_admin,
            &caller_id,
            req_data.old_password.as_deref(),
            req_data.password.as_deref().unwrap_or_default(),
        )?;
        Ok(Status::Ok)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Task join error"))?
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Cookie, Status};
    use serde_json::json;

    use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD, make_client, reset_backend_state};
    use crate::tests::fixtures::test_password;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn setup() -> rocket::local::blocking::Client {
        let _ = &*TEST_ENV;
        reset_backend_state();
        make_client()
    }

    fn cookie(id: &str, admin: bool) -> Cookie<'static> {
        let claims = crate::router::auth::Claims::new_user(id.to_string(), admin);
        let _ = crate::auth::users::add_session(id, &claims.jti);
        Cookie::new("jwt", claims.encode())
    }

    fn seed(id: &str, admin: bool, password: Option<&str>) {
        crate::auth::users::create_user(id, admin).expect("seed user");
        if let Some(pw) = password {
            let path = crate::auth::users::passwd_file_path();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("auth dir");
            }
            let mut store = crate::auth::password::PasswdFile::load(&path).expect("load passwd");
            store.set_password(id, pw).expect("set password");
        }
    }

    fn put_self(
        client: &rocket::local::blocking::Client,
        cookie: Cookie<'static>,
        body: &serde_json::Value,
    ) -> Status {
        client
            .put("/put/config/password")
            .cookie(cookie)
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch()
            .status()
    }

    #[test]
    fn admin_self_change_without_old_is_401() {
        let _g = lock();
        let client = setup();
        seed("s4e-root", true, Some(test_password("t-root-old").as_str()));
        let status = put_self(
            &client,
            cookie("s4e-root", true),
            &json!({ "password": test_password("t-root-new") }),
        );
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn admin_self_change_with_correct_old_is_200() {
        let _g = lock();
        let client = setup();
        seed(
            "s4e-root2",
            true,
            Some(test_password("t-root2-old").as_str()),
        );
        let status = put_self(
            &client,
            cookie("s4e-root2", true),
            &json!({ "password": test_password("t-root2-new"), "oldPassword": test_password("t-root2-old") }),
        );
        assert_eq!(status, Status::Ok);
        let store =
            crate::auth::password::PasswdFile::load(&crate::auth::users::passwd_file_path())
                .expect("load passwd");
        assert!(store.verify("s4e-root2", &test_password("t-root2-new")));
    }

    #[test]
    fn self_service_with_correct_old_password() {
        let _g = lock();
        let client = setup();
        seed("s4e-bob", false, Some(test_password("t-bob-old").as_str()));
        let status = put_self(
            &client,
            cookie("s4e-bob", false),
            &json!({ "password": test_password("t-bob-new"), "oldPassword": test_password("t-bob-old") }),
        );
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn wrong_old_password_is_401() {
        let _g = lock();
        let client = setup();
        seed(
            "s4e-carol",
            false,
            Some(test_password("t-carol-old").as_str()),
        );
        let status = put_self(
            &client,
            cookie("s4e-carol", false),
            &json!({ "password": test_password("t-carol-new"), "oldPassword": test_password("t-nope") }),
        );
        assert_eq!(status, Status::Unauthorized);
    }

    /// `address`, `port`, `allowRegistration`, and `trivialPasswords` are
    /// host-only like `bootstrapAdmin`: any presence in the API body is a
    /// 400 and changes nothing.
    #[test]
    fn put_config_rejects_host_only_network_and_policy_knobs() {
        let _g = lock();
        let client = setup();
        seed("s4e-root", true, None);
        let snapshot = || {
            let cfg = crate::model::config::APP_CONFIG
                .get()
                .expect("APP_CONFIG")
                .read()
                .expect("APP_CONFIG lock");
            (
                cfg.address.clone(),
                cfg.port,
                cfg.allow_registration,
                cfg.trivial_passwords,
            )
        };
        let before = snapshot();
        for body in [
            json!({ "address": "10.0.0.1" }),
            json!({ "port": 8888 }),
            json!({ "allowRegistration": true }),
            json!({ "trivialPasswords": true }),
        ] {
            let status = client
                .put("/put/config")
                .cookie(cookie("s4e-root", true))
                .header(ContentType::JSON)
                .body(body.to_string())
                .dispatch()
                .status();
            assert_eq!(status, Status::BadRequest, "host-only knob, body {body}");
        }
        assert_eq!(snapshot(), before, "rejected knobs must change nothing");
    }

    #[test]
    fn put_config_rejects_allow_registration_as_host_only() {
        let _g = lock();
        let client = setup();
        seed("s4e-root", true, None);
        let flag = || {
            crate::model::config::APP_CONFIG
                .get()
                .expect("APP_CONFIG")
                .read()
                .expect("APP_CONFIG lock")
                .allow_registration
        };
        assert!(!flag(), "default must be off");
        let status = client
            .put("/put/config")
            .cookie(cookie("s4e-root", true))
            .header(ContentType::JSON)
            .body(json!({ "allowRegistration": true }).to_string())
            .dispatch()
            .status();
        assert_eq!(
            status,
            Status::BadRequest,
            "host-only knob must be rejected"
        );
        assert!(!flag(), "rejected knob must not toggle on");
        // A patch that omits the field keeps the current value.
        let status = client
            .put("/put/config")
            .cookie(cookie("s4e-root", true))
            .header(ContentType::JSON)
            .body(json!({ "disableImg": false }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
        assert!(!flag(), "absent field must not touch the knob");
    }

    /// `authKey` is managed, never set: any presence in the API body is a
    /// 400 and changes nothing. Rotation goes through the reset endpoint.
    #[test]
    fn put_config_rejects_auth_key_as_managed() {
        let _g = lock();
        let client = setup();
        seed("s4e-root", true, None);
        {
            let mut cfg = crate::model::config::APP_CONFIG
                .get()
                .expect("APP_CONFIG")
                .write()
                .expect("APP_CONFIG lock");
            cfg.auth_key = Some("pre-existing-key".to_string());
        }
        let status = client
            .put("/put/config")
            .cookie(cookie("s4e-root", true))
            .header(ContentType::JSON)
            .body(json!({ "authKey": "smuggled-key" }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::BadRequest, "managed key must be rejected");
        assert_eq!(
            crate::model::config::APP_CONFIG
                .get()
                .expect("APP_CONFIG")
                .read()
                .expect("APP_CONFIG lock")
                .auth_key
                .as_deref(),
            Some("pre-existing-key"),
            "rejected key must not overwrite the current one"
        );
    }

    #[test]
    fn put_config_rejects_bootstrap_admin_as_host_only() {
        let _g = lock();
        let client = setup();
        seed("s4e-root", true, None);
        // The test harness opts into bootstrap mode; start explicitly off so
        // a leak would be observable.
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("APP_CONFIG lock")
            .bootstrap_admin = false;
        let flag = || {
            crate::model::config::APP_CONFIG
                .get()
                .expect("APP_CONFIG")
                .read()
                .expect("APP_CONFIG lock")
                .bootstrap_admin
        };
        assert!(!flag(), "start must be off");
        // `bootstrapAdmin` is host-only (flag / env / config file): any
        // presence in the API body is a 400 and changes nothing.
        for value in [json!(true), json!(false)] {
            let status = client
                .put("/put/config")
                .cookie(cookie("s4e-root", true))
                .header(ContentType::JSON)
                .body(json!({ "bootstrapAdmin": value }).to_string())
                .dispatch()
                .status();
            assert_eq!(status, Status::BadRequest, "host-only knob, value {value}");
        }
        assert!(!flag(), "rejected knob must not flip the flag");
        // A patch that omits the field still succeeds and keeps the value.
        let status = client
            .put("/put/config")
            .cookie(cookie("s4e-root", true))
            .header(ContentType::JSON)
            .body(json!({ "disableImg": false }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
        assert!(!flag(), "absent field must not touch the knob");
    }

    /// The 13 login-throttle knobs are host-only (TOML / `PICASU_*`, all
    /// require a restart: the throttle tracker is built once per process).
    /// Any presence in the API body is a 400 and changes nothing.
    #[test]
    fn put_config_rejects_login_throttle_knobs_as_host_only() {
        let _g = lock();
        let client = setup();
        seed("s4e-root", true, None);
        let snapshot = || {
            let cfg = crate::model::config::APP_CONFIG
                .get()
                .expect("APP_CONFIG")
                .read()
                .expect("APP_CONFIG lock");
            (
                cfg.trust_proxy_forwarded_for,
                cfg.login_ip_window_secs,
                cfg.login_ip_max_failures,
                cfg.login_ip_max_ban_secs,
                cfg.login_user_long_max_ban_secs,
            )
        };
        let before = snapshot();
        let bodies = [
            json!({ "trustProxyForwardedFor": true }),
            json!({ "loginIpWindowSecs": 100 }),
            json!({ "loginIpMaxFailures": 9 }),
            json!({ "loginIpBaseBanSecs": 20 }),
            json!({ "loginIpMaxBanSecs": 900 }),
            json!({ "loginIpLongWindowSecs": 1000 }),
            json!({ "loginIpLongMaxFailures": 90 }),
            json!({ "loginIpLongBaseBanSecs": 200 }),
            json!({ "loginIpLongMaxBanSecs": 9000 }),
            json!({ "loginUserLongWindowSecs": 1000 }),
            json!({ "loginUserLongMaxFailures": 91 }),
            json!({ "loginUserLongBaseBanSecs": 201 }),
            json!({ "loginUserLongMaxBanSecs": 9001 }),
        ];
        for body in bodies {
            let status = client
                .put("/put/config")
                .cookie(cookie("s4e-root", true))
                .header(ContentType::JSON)
                .body(body.to_string())
                .dispatch()
                .status();
            assert_eq!(status, Status::BadRequest, "host-only knob, body {body}");
        }
        assert_eq!(snapshot(), before, "rejected knobs must change nothing");
    }

    #[test]
    fn empty_new_password_is_400() {
        let _g = lock();
        let client = setup();
        seed("s4e-root", true, None);
        let status = put_self(
            &client,
            cookie("s4e-root", true),
            &json!({ "password": "   " }),
        );
        assert_eq!(status, Status::BadRequest);
    }
}
