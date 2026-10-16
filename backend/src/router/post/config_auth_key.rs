// src/router/post/config_auth_key.rs

use rocket::post;
use rocket::serde::json::Json;
use serde::Serialize;

use crate::openapi_components::Unauthorized;
use crate::router::AppResult;
use crate::router::auth::GuardAuth;

/// Result of rotating the JWT signing key. Always true after a successful
/// reset; the client drives its key-state display off this (the key itself
/// is never returned).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthKeyResetResponse {
    pub has_auth_key: bool,
}

/// Rotate the JWT signing key.
///
/// Generates a new strong random key, persists it to the owner-only
/// secrets file, and installs it in memory. All previously issued tokens
/// stop verifying, so every session is invalidated by construction.
/// Overrides from `PICASU_AUTH_KEY` still win at the next boot (they are
/// never persisted).
///
/// Errors: 401 missing or invalid admin credentials — 500 the new key
/// could not be stored.
#[utoipa::path(
        tag = "config",
        responses(
            (status = 200, description = "Signing key rotated", body = AuthKeyResetResponse),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/config/auth-key/reset")]
pub fn reset_auth_key_handler(_auth: GuardAuth) -> AppResult<Json<AuthKeyResetResponse>> {
    use crate::auth::secret_key;
    use crate::model::config::{APP_CONFIG, generate_secret_key};
    use crate::router::{AppError, ErrorKind};

    let key = generate_secret_key();
    secret_key::store_secret_key(&key).map_err(|e| AppError::from_err(ErrorKind::IO, e))?;
    APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .write()
        .expect("lock poisoned")
        .auth_key = Some(key);
    log::info!("auth: signing key reset; all sessions invalidated");
    Ok(Json(AuthKeyResetResponse { has_auth_key: true }))
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Cookie, Status};
    use serde_json::{Value, json};

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

    fn reset(client: &rocket::local::blocking::Client, cookie: Option<Cookie<'static>>) -> Status {
        let mut req = client
            .post("/post/config/auth-key/reset")
            .header(ContentType::JSON);
        if let Some(c) = cookie {
            req = req.cookie(c);
        }
        req.dispatch().status()
    }

    fn list_status(client: &rocket::local::blocking::Client, cookie: Cookie<'static>) -> Status {
        client.get("/get/users").cookie(cookie).dispatch().status()
    }

    /// Reset rotates the signing key: the response confirms it, the old
    /// session stops verifying, a fresh session verifies, and the stored
    /// key matches memory.
    #[test]
    fn reset_rotates_key_and_invalidates_old_sessions() {
        let _g = lock();
        let client = setup();
        seed("s4k-root", true);
        let old = cookie("s4k-root", true);
        assert_eq!(list_status(&client, old.clone()), Status::Ok);

        let resp = client
            .post("/post/config/auth-key/reset")
            .cookie(old.clone())
            .header(ContentType::JSON)
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body: Value = serde_json::from_str(&resp.into_string().expect("body")).expect("json");
        assert_eq!(body["hasAuthKey"], true);

        assert_eq!(
            list_status(&client, old),
            Status::Unauthorized,
            "tokens minted under the old key must stop verifying"
        );
        let fresh = cookie("s4k-root", true);
        assert_eq!(
            list_status(&client, fresh),
            Status::Ok,
            "sessions minted after reset must verify"
        );

        let stored = crate::auth::secret_key::load_secret_key().expect("load secrets file");
        let memory = crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .read()
            .expect("APP_CONFIG lock")
            .auth_key
            .clone();
        assert_eq!(stored, memory, "file and memory must agree after reset");
        assert!(stored.is_some(), "reset must persist a key");
    }

    /// Two resets mint different keys: rotation is random, not static.
    #[test]
    fn consecutive_resets_mint_different_keys() {
        let _g = lock();
        let client = setup();
        seed("s4k-root", true);
        assert_eq!(reset(&client, Some(cookie("s4k-root", true))), Status::Ok);
        let first = crate::auth::secret_key::load_secret_key().expect("load first");
        assert_eq!(reset(&client, Some(cookie("s4k-root", true))), Status::Ok);
        let second = crate::auth::secret_key::load_secret_key().expect("load second");
        assert_ne!(first, second, "each reset must mint a fresh random key");
    }

    /// The reset route is admin-only like the rest of the config surface.
    #[test]
    fn reset_requires_admin() {
        let _g = lock();
        let client = setup();
        seed("s4k-root", true);
        seed("s4k-bob", false);
        assert_eq!(reset(&client, None), Status::Unauthorized);
        assert_eq!(
            reset(&client, Some(cookie("s4k-bob", false))),
            Status::Unauthorized
        );
        assert_eq!(
            crate::auth::secret_key::load_secret_key().expect("load"),
            None,
            "refused reset must store nothing"
        );
    }
}
