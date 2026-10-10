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
    pub address: Option<String>,
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
    pub auth_key: Option<String>,
}

/// Patch the server configuration with the posted fields.
///
/// Every field is optional and a field absent from the body keeps its current
/// value. The only validation is on `uploadFolder`, which must be a relative
/// path free of `..` components; on success `config.toml` is rewritten, the
/// in-memory configuration is replaced and the filesystem watcher is reloaded.
///
/// Corner cases: `authKey` is trimmed, and clearing or replacing it moves JWT
/// signing onto a new key, invalidating every token issued under the previous
/// one. `uploadFolder` and `maxUploadSize` take an empty string to reset to
/// their defaults, `uploads` and `100MiB`. `address` and `port` are stored but
/// the listener is not rebound, so they take effect only after a restart.
///
/// Errors: 400 `uploadFolder` is absolute or contains `..`, or the body cannot
/// be parsed — 401 missing or invalid admin credentials; share tokens are not
/// accepted — 405 read-only mode — 500 config write failure.
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

/// Change the caller's own password.
///
/// This is the legacy single-password route, reworked onto the user store:
/// it changes the password of the authenticated caller only. An admin may
/// set their own password without `oldPassword`; a non-admin must present
/// the correct `oldPassword`. The new `password` is trimmed and must be
/// non-empty after trimming (there is no "clear to open": open mode is
/// store-empty only). To change another user's password, use
/// `PUT /put/users/password`.
///
/// Corner cases: Any authenticated user reaches this route (share tokens
/// are rejected); unauthenticated callers are denied at dispatch. A missing
/// (`None`) or blank `password` is 400.
///
/// Errors: 400 missing or blank `password`, or the body cannot be parsed —
/// 401 missing or invalid credentials, or wrong `oldPassword`; share tokens
/// are not accepted — 405 read-only mode — 500 storage failure.
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
            req_data.password.as_deref().unwrap_or(""),
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
        let token = crate::router::auth::Claims::new_user(id.to_string(), admin).encode();
        Cookie::new("jwt", token)
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

    fn put_legacy(
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
    fn legacy_admin_sets_own_password_without_old() {
        let _g = lock();
        let client = setup();
        seed("s4e-root", true, Some("s4e-root-old"));
        let status = put_legacy(
            &client,
            cookie("s4e-root", true),
            &json!({ "password": "s4e-root-new" }),
        );
        assert_eq!(status, Status::Ok);
        let store =
            crate::auth::password::PasswdFile::load(&crate::auth::users::passwd_file_path())
                .expect("load passwd");
        assert!(store.verify("s4e-root", "s4e-root-new"));
    }

    #[test]
    fn legacy_self_service_with_correct_old_password() {
        let _g = lock();
        let client = setup();
        seed("s4e-bob", false, Some("s4e-bob-old"));
        let status = put_legacy(
            &client,
            cookie("s4e-bob", false),
            &json!({ "password": "s4e-bob-new", "oldPassword": "s4e-bob-old" }),
        );
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn legacy_wrong_old_password_is_401() {
        let _g = lock();
        let client = setup();
        seed("s4e-carol", false, Some("s4e-carol-old"));
        let status = put_legacy(
            &client,
            cookie("s4e-carol", false),
            &json!({ "password": "s4e-carol-new", "oldPassword": "nope" }),
        );
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn legacy_empty_new_password_is_400() {
        let _g = lock();
        let client = setup();
        seed("s4e-root", true, None);
        let status = put_legacy(
            &client,
            cookie("s4e-root", true),
            &json!({ "password": "   " }),
        );
        assert_eq!(status, Status::BadRequest);
    }
}
