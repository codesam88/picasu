use rocket::post;
use rocket::serde::json::Json;

use crate::model::config::APP_CONFIG;
use crate::openapi_components::Unauthorized;
use crate::router::auth::Claims;
use crate::router::{AppError, AppResult, ErrorKind};

/// Sign in with the account password and return a 14-day admin JWT.
///
/// The posted password is trimmed and compared against the configured one; a
/// match returns the signed admin token as a JSON string. Tokens are signed
/// with `authKey`, or with a secret generated once per process when `authKey`
/// is unset, so changing `authKey` invalidates every token issued under the
/// previous one.
///
/// Corner cases: While no password is configured, sign-in succeeds without
/// one: every input, including an empty string, is accepted.
///
/// Errors: 401 password does not match the configured one.
#[utoipa::path(
        tag = "auth",
        request_body = String,
        responses(
            (status = 200, description = "JWT token", body = String),
            (status = 401, response = Unauthorized),
        )
    )
]
#[post("/post/authenticate", data = "<password>")]
pub fn authenticate(password: Json<String>) -> AppResult<Json<String>> {
    // Trim input password to match storage behavior
    let input_password = password.into_inner().trim().to_string();

    let current_password = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned")
        .password
        .clone();

    let is_valid = match current_password {
        Some(pwd) => input_password == pwd,
        None => true,
    };

    if is_valid {
        let token = Claims::new_admin().encode();
        Ok(Json(token))
    } else {
        Err(AppError::new(ErrorKind::Auth, "Invalid password").context("Authentication failed"))
    }
}
