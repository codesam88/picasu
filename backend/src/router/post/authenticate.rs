use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::auth::password::{PasswdFile, password_iterations, verify_password};
use crate::auth::users::{self, validate_user_id};
use crate::error::ResultExt;
use crate::openapi_components::Unauthorized;
use crate::router::auth::Claims;
use crate::router::{AppError, AppResult, ErrorKind};

/// Request body for `POST /post/authenticate`: a user login object.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub user_id: String,
    pub password: String,
}

/// Run one PBKDF verification against a throwaway hash. Always false;
/// exposed for tests to assert the unknown-user path pays a KDF round.
///
/// Salt, expected hash, and password are freshly generated per call, so
/// no fixed constant exists for readers or scanners to mistake for a
/// credential. Unpredictability is not required (verification fails
/// regardless); `rand::random` is used because it is already the
/// crate's randomness source, not because the dummy needs strength.
pub(crate) fn run_dummy_verify() -> bool {
    verify_password(
        &format!("{:x}-{:x}", rand::random::<u64>(), rand::random::<u64>()),
        &crate::auth::password::PasswordHash {
            salt: rand::random(),
            hash: rand::random(),
            iterations: password_iterations(),
        },
    )
}

/// Verify `password` for `user_id` against the user and password stores and
/// mint a user-bound 14-day JWT. Must run on a blocking thread: password
/// hashing is CPU-heavy.
///
/// The password input is trimmed once and the trimmed (canonical) form is
/// verified; callers writing new passwords must store the trimmed form too
/// so write-then-login stays consistent.
///
/// # Errors
/// Returns 401 when the user is unknown or the password is wrong, and 500
/// when either store cannot be read.
fn login_sync(user_id: &str, password: &str) -> AppResult<String> {
    let canonical = password.trim();
    let record =
        users::get_user(user_id).map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
    let Some(record) = record else {
        // Pay one KDF round so unknown users are not measurably faster
        // than wrong passwords (user-enumeration timing oracle).
        let _ = run_dummy_verify();
        return Err(
            AppError::new(ErrorKind::Auth, "Invalid credentials").context("Authentication failed")
        );
    };
    let store = PasswdFile::load(&users::passwd_file_path())
        .map_err(|e| AppError::from_err(ErrorKind::IO, e))?;
    if !store.verify(user_id, canonical) {
        return Err(
            AppError::new(ErrorKind::Auth, "Invalid credentials").context("Authentication failed")
        );
    }
    Ok(Claims::new_user(user_id.to_string(), record.admin).encode())
}

/// Sign in and return a 14-day user-bound JWT.
///
/// A `{ userId, password }` object verifies against the user and password
/// stores and mints a token carrying the user's id and admin flag. Tokens
/// are signed with `authKey`, or with a secret generated once per process
/// when `authKey` is unset, so changing `authKey` invalidates every token
/// issued under the previous one.
///
/// Corner cases: While no users exist and `--bootstrap-admin` is set,
/// sign-in succeeds without credentials: every parseable login object is
/// accepted (open first-run mode).
///
/// Errors: 400 the login `userId` is empty or longer than 64 characters, or
/// the body does not parse as a login object — 401 unknown user or wrong
/// password.
#[utoipa::path(
        tag = "auth",
        request_body = LoginRequest,
        responses(
            (status = 200, description = "JWT token", body = String),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/authenticate", data = "<body>")]
pub async fn authenticate(body: Json<LoginRequest>) -> AppResult<Json<String>> {
    // Open bootstrap mode: no users and the `--bootstrap-admin` flag set
    // accept any parseable login object without touching the stores.
    let open_mode = users::user_count().map_err(|e| AppError::from_err(ErrorKind::Database, e))?
        == 0
        && crate::router::auth::bootstrap_open();
    if open_mode {
        let token = Claims::new_user("admin".to_string(), true).encode();
        return Ok(Json(token));
    }

    let login = body.into_inner();
    let user_id = validate_user_id(&login.user_id)?;
    let password = login.password;
    let token = spawn_blocking(move || login_sync(&user_id, &password))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
    Ok(Json(token))
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Status};
    use serde_json::json;

    use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD, make_client, reset_backend_state};
    use crate::tests::fixtures::{test_password, test_password_padded};

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

    fn set_bootstrap_admin(enabled: bool) {
        let mut config = crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG set")
            .write()
            .expect("APP_CONFIG lock");
        config.bootstrap_admin = enabled;
    }

    fn create_user_with_password(id: &str, admin: bool, password: &str) {
        crate::auth::users::create_user(id, admin).expect("create user");
        let path = crate::auth::users::passwd_file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create auth dir");
        }
        let mut store = crate::auth::password::PasswdFile::load(&path).expect("load passwd");
        store.set_password(id, password).expect("set password");
    }

    fn decode_claims(token: &str) -> crate::router::auth::Claims {
        let token = token.trim_matches('"');
        let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
        crate::router::auth::decode_typed(token, &validation).expect("decode token")
    }

    #[test]
    fn authenticate_login_mints_user_bound_token() {
        let _g = lock();
        let client = setup();
        create_user_with_password("s3c-alice", true, &test_password("t-alice"));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "s3c-alice", "password": test_password("t-alice") }).to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("token body");
        let claims = decode_claims(&body);
        match claims.role {
            crate::router::auth::Role::User { id, admin } => {
                assert_eq!(id, "s3c-alice");
                assert!(admin);
            }
            crate::router::auth::Role::Share(_) => panic!("expected user role"),
        }
    }

    #[test]
    fn authenticate_login_wrong_and_unknown_user_are_401() {
        let _g = lock();
        let client = setup();
        create_user_with_password("s3c-bob", false, &test_password("t-bob"));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "s3c-bob", "password": test_password("t-wrong") }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "s3c-ghost", "password": test_password("t-ghost") }).to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
    }

    #[test]
    fn bare_string_body_is_422_on_empty_store_flag_off() {
        let _g = lock();
        let client = setup();
        set_bootstrap_admin(false);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!(test_password("t-closed")).to_string())
            .dispatch();
        // Rocket's `Json` guard maps well-formed JSON of the wrong shape
        // (string vs. login object) to 422, not 400.
        assert_eq!(resp.status(), Status::UnprocessableEntity);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
    }

    #[test]
    fn bare_string_body_is_422_on_empty_store_flag_on() {
        let _g = lock();
        let client = setup();
        set_bootstrap_admin(true);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        // The bare-string shape no longer parses as `LoginRequest`, so even
        // the open window answers 400: there is no legacy bootstrap path.
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!(test_password("t-closed")).to_string())
            .dispatch();
        // Rocket's `Json` guard maps well-formed JSON of the wrong shape
        // (string vs. login object) to 422, not 400.
        assert_eq!(resp.status(), Status::UnprocessableEntity);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
    }

    #[test]
    fn bare_string_body_is_422_on_non_empty_store() {
        let _g = lock();
        let client = setup();
        create_user_with_password("s3d-carol", true, &test_password("t-carol"));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!(test_password("t-s3d-str")).to_string())
            .dispatch();
        // Rocket's `Json` guard maps well-formed JSON of the wrong shape
        // (string vs. login object) to 422, not 400.
        assert_eq!(resp.status(), Status::UnprocessableEntity);
    }

    #[test]
    fn empty_store_flag_off_login_is_401_and_creates_nothing() {
        let _g = lock();
        let client = setup();
        set_bootstrap_admin(false);
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "s3e-bob", "password": test_password("t-wrong") }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        assert!(
            crate::auth::users::get_user("s3e-bob")
                .expect("get")
                .is_none()
        );
    }

    #[test]
    fn empty_store_flag_on_login_opens_without_creating_user() {
        let _g = lock();
        let client = setup();
        set_bootstrap_admin(true);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "anything", "password": test_password("t-open") }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("token body");
        let claims = decode_claims(&body);
        match claims.role {
            crate::router::auth::Role::User { id, admin } => {
                assert_eq!(id, "admin");
                assert!(admin);
            }
            crate::router::auth::Role::Share(_) => panic!("expected user role"),
        }
        // Open mode mints a token without enrolling anybody.
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
    }

    #[test]
    fn login_invalid_id_is_400() {
        let _g = lock();
        let client = setup();
        set_bootstrap_admin(false);
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "   ", "password": test_password("t-s3e-str") }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::BadRequest);
    }

    #[test]
    fn login_trims_padded_password() {
        let _g = lock();
        let client = setup();
        create_user_with_password("s4-trim-user", false, &test_password("t-trim"));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "s4-trim-user", "password": test_password_padded("t-trim") })
                    .to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
    }

    #[test]
    fn unknown_user_dummy_verify_runs_pbkdf2() {
        let start = std::time::Instant::now();
        assert!(!super::run_dummy_verify());
        // Timing threshold only holds at production effort: under the
        // test-only iteration default/override the round is deliberately
        // cheap, so only correctness is asserted there.
        if crate::auth::password::password_iterations() == crate::auth::password::DEFAULT_ITERATIONS
        {
            assert!(
                start.elapsed().as_millis() > 50,
                "dummy verify must cost a PBKDF2 round"
            );
        }
    }
}
