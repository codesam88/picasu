use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::auth::password::{
    DEFAULT_ITERATIONS, HASH_LEN, PasswdFile, PasswordHash, SALT_LEN, verify_password,
};
use crate::auth::users::{self, validate_user_id};
use crate::error::ResultExt;
use crate::model::config::APP_CONFIG;
use crate::openapi_components::Unauthorized;
use crate::router::auth::Claims;
use crate::router::{AppError, AppResult, ErrorKind};

/// User login body for `POST /post/authenticate`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub user_id: String,
    pub password: String,
}

/// Request body for `POST /post/authenticate`: either a user login object
/// or the legacy bare-string bootstrap path. Untagged so a JSON string and
/// a JSON object both parse on the same route.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum AuthenticateRequest {
    Legacy(String),
    Login(LoginRequest),
}

/// Fixed dummy hash for the unknown-user path: all-zero salt with the
/// production iteration count, so an unknown user costs one PBKDF2 round
/// like a wrong password and leaks no timing signal.
fn dummy_hash() -> PasswordHash {
    PasswordHash {
        salt: [0u8; SALT_LEN],
        hash: [0u8; HASH_LEN],
        iterations: DEFAULT_ITERATIONS,
    }
}

/// Run one PBKDF2 verification against the fixed dummy hash. Always false;
/// exposed for tests to assert the unknown-user path pays a KDF round.
pub(crate) fn run_dummy_verify() -> bool {
    verify_password("dummy-unknown-user-password", &dummy_hash())
}

/// Verify `password` for `user_id` against the user and password stores and
/// mint a user-bound 14-day JWT. Must run on a blocking thread: password
/// hashing is CPU-heavy.
///
/// The password input is trimmed once and the trimmed (canonical) form is
/// verified, matching the legacy semantics that trimmed everywhere; callers
/// writing new passwords must store the trimmed form too so
/// write-then-login stays consistent.
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

/// Migrate the legacy single-password config into the user store: when no
/// users exist and the trimmed input matches the configured legacy password,
/// create user `admin` and store the password hash for it. Must run on a
/// blocking thread: password hashing is CPU-heavy.
///
/// The input is trimmed once and the trimmed form is both compared and
/// hashed, matching the previous verbatim-compare-after-trim semantics.
///
/// # Errors
/// Returns 401 when the store is already non-empty (the legacy path is
/// bootstrap-only) or when the input does not match the configured legacy
/// password, and 500 when either store cannot be written.
fn ensure_migrated_from_legacy(password: &str) -> AppResult<()> {
    let non_empty = users::user_count().map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
    if non_empty > 0 {
        return Err(AppError::new(ErrorKind::Auth, "Legacy sign-in is disabled")
            .context("Authentication failed"));
    }
    let trimmed = password.trim();
    let legacy = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned")
        .password
        .clone();
    match legacy {
        Some(expected) if !expected.is_empty() && trimmed == expected => {}
        _ => {
            return Err(
                AppError::new(ErrorKind::Auth, "Invalid password").context("Authentication failed")
            );
        }
    }
    users::create_user("admin", true).map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
    let path = users::passwd_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::from_err(ErrorKind::IO, e.into()))?;
    }
    let mut store = PasswdFile::load(&path).map_err(|e| AppError::from_err(ErrorKind::IO, e))?;
    store
        .set_password("admin", trimmed)
        .map_err(|e| AppError::from_err(ErrorKind::IO, e))?;
    Ok(())
}

/// Sign in and return a 14-day user-bound JWT.
///
/// A `{ userId, password }` object verifies against the user and password
/// stores and mints a token carrying the user's id and admin flag. A bare
/// JSON string is the legacy bootstrap path: while the user store is empty
/// it verifies the configured legacy password and migrates it onto user
/// `admin`, and it is rejected once any user exists. Tokens are signed with
/// `authKey`, or with a secret generated once per process when `authKey` is
/// unset, so changing `authKey` invalidates every token issued under the
/// previous one.
///
/// Corner cases: While no users exist and no legacy password is configured,
/// sign-in succeeds without credentials: every parseable input is accepted
/// (open first-run mode).
///
/// Errors: 400 the login `userId` is empty or longer than 64 characters, or
/// the body parses as neither shape — 401 unknown user, wrong password, or
/// legacy path used after bootstrap.
#[utoipa::path(
        tag = "auth",
        request_body = AuthenticateRequest,
        responses(
            (status = 200, description = "JWT token", body = String),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/authenticate", data = "<body>")]
pub async fn authenticate(body: Json<AuthenticateRequest>) -> AppResult<Json<String>> {
    // Open first-run mode: no users and no legacy password accept any
    // parseable body without touching the stores.
    let open_mode = users::user_count().map_err(|e| AppError::from_err(ErrorKind::Database, e))?
        == 0
        && APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned")
            .password
            .is_none();
    if open_mode {
        let token = Claims::new_user("admin".to_string(), true).encode();
        return Ok(Json(token));
    }

    match body.into_inner() {
        AuthenticateRequest::Login(login) => {
            let user_id = validate_user_id(&login.user_id)?;
            let password = login.password;
            let token = spawn_blocking(move || login_sync(&user_id, &password))
                .await
                .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
            Ok(Json(token))
        }
        AuthenticateRequest::Legacy(password) => {
            let token = spawn_blocking(move || -> AppResult<String> {
                // The trimmed form is canonical for comparison, storage, and
                // the follow-up verification alike.
                let canonical = password.trim().to_string();
                ensure_migrated_from_legacy(&canonical)?;
                login_sync("admin", &canonical)
            })
            .await
            .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
            Ok(Json(token))
        }
    }
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Status};
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

    fn set_legacy_password(password: Option<&str>) {
        let mut config = crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG set")
            .write()
            .expect("APP_CONFIG lock");
        config.password = password.map(str::to_string);
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
        set_legacy_password(Some("s3c-legacy"));
        create_user_with_password("s3c-alice", true, "s3c-correct");
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "s3c-alice", "password": "s3c-correct" }).to_string())
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
        set_legacy_password(None);
    }

    #[test]
    fn authenticate_login_wrong_and_unknown_user_are_401() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some("s3c-legacy"));
        create_user_with_password("s3c-bob", false, "s3c-correct");
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "s3c-bob", "password": "wrong" }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "s3c-ghost", "password": "anything" }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
        set_legacy_password(None);
    }

    #[test]
    fn legacy_string_migrates_to_admin_user() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some("s3d-legacy-secret"));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!("s3d-legacy-secret").to_string())
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
        // The migrated user exists in both stores and its token is live.
        let record = crate::auth::users::get_user("admin").expect("get migrated user");
        assert_eq!(record, Some(crate::auth::users::UserRecord { admin: true }));
        let passwd =
            crate::auth::password::PasswdFile::load(&crate::auth::users::passwd_file_path())
                .expect("load passwd");
        assert!(passwd.verify("admin", "s3d-legacy-secret"));
        let token = body.trim_matches('"').to_string();
        let resp = client
            .get("/get/index/status")
            .cookie(rocket::http::Cookie::new("jwt", token))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        // A second legacy attempt is rejected once the store is non-empty.
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!("s3d-legacy-secret").to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
        set_legacy_password(None);
    }

    #[test]
    fn legacy_string_rejected_once_store_non_empty() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some("s3d-legacy-secret"));
        create_user_with_password("s3d-carol", true, "s3d-carol-pw");
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!("s3d-legacy-secret").to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
        set_legacy_password(None);
    }

    #[test]
    fn login_trims_padded_password() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some("s4-trim-legacy"));
        create_user_with_password("s4-trim-user", false, "s4-secret");
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "s4-trim-user", "password": "  s4-secret  " }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        set_legacy_password(None);
    }

    #[test]
    fn unknown_user_dummy_verify_runs_pbkdf2() {
        let start = std::time::Instant::now();
        assert!(!super::run_dummy_verify());
        assert!(
            start.elapsed().as_millis() > 50,
            "dummy verify must cost a PBKDF2 round"
        );
    }
}
