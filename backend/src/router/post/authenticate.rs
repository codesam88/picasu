use rocket::post;
use rocket::serde::json::Json;

use crate::model::config::APP_CONFIG;
use crate::router::auth::Claims;
use crate::router::{AppError, AppResult, ErrorKind};

#[utoipa::path(
        post,
        path = "/post/authenticate",
        request_body = String,
        responses(
            (status = 200, description = "JWT token", body = String),
            (status = 401, description = "Invalid password"),
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
