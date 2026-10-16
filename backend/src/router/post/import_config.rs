// src/router/post/import_config.rs

use log::error;
use rocket::http::Status;
use rocket::post;
use rocket::serde::json::Json;

use crate::error::{AppError, ErrorKind};
use crate::model::config::{APP_CONFIG, AppConfig};
use crate::openapi_components::Unauthorized;
use crate::router::AppResult;
use crate::router::auth::GuardAuth;

/// Replace the server configuration with the posted one.
///
/// The body is a complete `AppConfig` rather than a patch. Before writing,
/// `imagePath` is trimmed of whitespace and quotes (an empty value becomes
/// `null`) and `uploadFolder` is trimmed. A posted `authKey` is ignored —
/// the current signing key is kept and can only be rotated through
/// `POST /post/config/auth-key/reset` — so restoring an older export never
/// smuggles a key in. On success `config.toml` is rewritten, the in-memory
/// configuration is replaced and the filesystem watcher reloaded.
///
/// Corner cases: The posted configuration is otherwise accepted as-is:
/// unlike `PUT /put/config` it applies no validation, so an `uploadFolder`
/// that is absolute or contains `..` is stored here. A failed write is a
/// 500 and leaves the running configuration untouched.
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
    let mut incoming = file.into_inner();
    incoming.auth_key.clone_from(
        &APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned")
            .auth_key,
    );
    match AppConfig::update(incoming) {
        Ok(()) => Ok(Status::Ok),
        Err(e) => {
            error!("Import failed: {e}");
            Err(AppError::from_err(ErrorKind::Internal, e))
        }
    }
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
        let claims = crate::router::auth::Claims::new_user(id.to_string(), admin);
        let _ = crate::auth::users::add_session(id, &claims.jti);
        Cookie::new("jwt", claims.encode())
    }

    fn seed(id: &str, admin: bool) {
        crate::auth::users::create_user(id, admin).expect("seed user");
    }

    fn current_key() -> Option<String> {
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .read()
            .expect("APP_CONFIG lock")
            .auth_key
            .clone()
    }

    /// A smuggled `authKey` in an import body is ignored: the current
    /// signing key is kept, the rest of the document still applies.
    /// Rotation goes through the reset endpoint only.
    #[test]
    fn import_ignores_smuggled_auth_key() {
        let _g = lock();
        let client = setup();
        seed("s4i-root", true);
        let admin = cookie("s4i-root", true);
        assert_eq!(
            client
                .post("/post/config/auth-key/reset")
                .cookie(admin.clone())
                .header(ContentType::JSON)
                .dispatch()
                .status(),
            Status::Ok,
            "reset must establish the current key first"
        );
        let before = current_key();
        assert!(before.is_some(), "a key must be configured");
        // The reset invalidated the pre-reset session: mint a fresh one and
        // capture the folder to restore afterwards.
        let admin = cookie("s4i-root", true);
        let orig_folder = crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .read()
            .expect("APP_CONFIG lock")
            .upload_folder
            .clone();

        // Import the live configuration with a smuggled key, so the only
        // behavioral difference is the key itself; the rest must apply.
        let mut body = serde_json::to_value(
            crate::model::config::APP_CONFIG
                .get()
                .expect("APP_CONFIG")
                .read()
                .expect("APP_CONFIG lock")
                .clone(),
        )
        .expect("config json");
        body["authKey"] = json!("smuggled-key");
        body["uploadFolder"] = json!("imported_uploads");
        let status = client
            .post("/post/config/import")
            .cookie(admin.clone())
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
        assert_eq!(current_key(), before, "import must keep the current key");
        assert_eq!(
            crate::auth::secret_key::load_secret_key().expect("load"),
            before,
            "import must not persist the smuggled key either"
        );
        assert_eq!(
            crate::model::config::APP_CONFIG
                .get()
                .expect("APP_CONFIG")
                .read()
                .expect("APP_CONFIG lock")
                .upload_folder,
            "imported_uploads",
            "the rest of the document must still apply"
        );

        // Restore global state for later tests in this binary.
        let status = client
            .put("/put/config")
            .cookie(admin)
            .header(ContentType::JSON)
            .body(json!({ "uploadFolder": orig_folder }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
    }
}
