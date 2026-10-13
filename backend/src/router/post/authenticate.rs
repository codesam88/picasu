use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::auth::password::{PasswdFile, password_iterations, verify_password};
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

/// Read the configured legacy single password, if any.
fn legacy_password() -> Option<String> {
    APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned")
        .password
        .clone()
}

/// Register `id` as the first admin with the given (already trimmed)
/// password hash. Must run on a blocking thread: password hashing is
/// CPU-heavy.
///
/// The store-emptiness re-check and the creation happen under one process
/// lock: the redb write and the password-file write cannot share a
/// transaction, so without the lock two concurrent bootstraps could each
/// create a different first admin.
///
/// # Errors
/// Returns 401 when the store is already non-empty (bootstrap-only), and 500
/// when either store cannot be written.
fn bootstrap_user(id: &str, trimmed_password: &str) -> AppResult<()> {
    static BOOTSTRAP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = BOOTSTRAP_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let non_empty = users::user_count().map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
    if non_empty > 0 {
        return Err(AppError::new(ErrorKind::Auth, "Legacy sign-in is disabled")
            .context("Authentication failed"));
    }
    users::create_user(id, true).map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
    let path = users::passwd_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::from_err(ErrorKind::IO, e.into()))?;
    }
    let mut store = PasswdFile::load(&path).map_err(|e| AppError::from_err(ErrorKind::IO, e))?;
    store
        .set_password(id, trimmed_password)
        .map_err(|e| AppError::from_err(ErrorKind::IO, e))?;
    log::info!("enrollment: first admin bootstrapped as {id}");
    Ok(())
}

/// Migrate the legacy single-password config into the user store: when no
/// users exist and the trimmed input matches the configured legacy password,
/// register the claimed id as the first admin. The bare-string path passes
/// `"admin"`, preserving the previous migration target.
///
/// Must run on a blocking thread: password hashing is CPU-heavy.
///
/// # Errors
/// Returns 401 when the store is already non-empty (the legacy path is
/// bootstrap-only) or when the input does not match the configured legacy
/// password, and 500 when either store cannot be written.
fn ensure_migrated_from_legacy(user_id: &str, password: &str) -> AppResult<()> {
    let trimmed = password.trim();
    match legacy_password() {
        Some(expected) if !expected.is_empty() && trimmed == expected => {}
        _ => {
            return Err(
                AppError::new(ErrorKind::Auth, "Invalid password").context("Authentication failed")
            );
        }
    }
    bootstrap_user(user_id, trimmed)
}

/// Sign in and return a 14-day user-bound JWT.
///
/// A `{ userId, password }` object verifies against the user and password
/// stores and mints a token carrying the user's id and admin flag. On a
/// legacy deployment (empty store + configured legacy password) the first
/// login with the legacy password registers the claimed `userId` as the
/// first admin. A bare
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
    // Open bootstrap mode: no users, the `--bootstrap-admin` flag set, and no
    // legacy password accept any parseable body without touching the stores.
    let open_mode = users::user_count().map_err(|e| AppError::from_err(ErrorKind::Database, e))?
        == 0
        && crate::router::auth::bootstrap_open();
    if open_mode {
        let token = Claims::new_user("admin".to_string(), true).encode();
        return Ok(Json(token));
    }

    match body.into_inner() {
        AuthenticateRequest::Login(login) => {
            let user_id = validate_user_id(&login.user_id)?;
            let password = login.password;
            let token = spawn_blocking(move || -> AppResult<String> {
                // Legacy deployment (empty store + configured password):
                // the first login with the legacy password registers the
                // claimed id as the first admin. Otherwise the normal path
                // below answers 401 for the unknown user.
                if users::user_count().map_err(|e| AppError::from_err(ErrorKind::Database, e))? == 0
                {
                    let canonical = password.trim().to_string();
                    match legacy_password() {
                        Some(expected) if !expected.is_empty() && canonical == expected => {
                            bootstrap_user(&user_id, &canonical)?;
                        }
                        _ => {}
                    }
                }
                login_sync(&user_id, &password)
            })
            .await
            .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
            Ok(Json(token))
        }
        AuthenticateRequest::Legacy(password) => {
            let token = spawn_blocking(move || -> AppResult<String> {
                // The trimmed form is canonical for comparison, storage, and
                // the follow-up verification alike. The bare-string path
                // keeps the historical `"admin"` migration target.
                let canonical = password.trim().to_string();
                ensure_migrated_from_legacy("admin", &canonical)?;
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
        set_legacy_password(Some(&test_password("t-legacy")));
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
        set_legacy_password(None);
    }

    #[test]
    fn authenticate_login_wrong_and_unknown_user_are_401() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some(&test_password("t-legacy")));
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
        set_legacy_password(None);
    }

    #[test]
    fn legacy_string_migrates_to_admin_user() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some(&test_password("t-s3d-legacy")));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!(test_password("t-s3d-legacy")).to_string())
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
        assert!(passwd.verify("admin", &test_password("t-s3d-legacy")));
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
            .body(json!(test_password("t-s3d-legacy")).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
        set_legacy_password(None);
    }

    #[test]
    fn legacy_string_rejected_once_store_non_empty() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some(&test_password("t-s3d-legacy")));
        create_user_with_password("s3d-carol", true, &test_password("t-carol"));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!(test_password("t-s3d-legacy")).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
        set_legacy_password(None);
    }

    #[test]
    fn legacy_login_registers_claimed_id_as_admin() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some(&test_password("t-s3e-legacy")));
        // First login with the legacy password claims a new id: it is
        // registered as the first admin instead of hardcoded `admin`.
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "s3e-alice", "password": test_password("t-s3e-legacy") })
                    .to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("token body");
        let claims = decode_claims(&body);
        match claims.role {
            crate::router::auth::Role::User { id, admin } => {
                assert_eq!(id, "s3e-alice");
                assert!(admin);
            }
            crate::router::auth::Role::Share(_) => panic!("expected user role"),
        }
        let record = crate::auth::users::get_user("s3e-alice").expect("get bootstrapped user");
        assert_eq!(record, Some(crate::auth::users::UserRecord { admin: true }));
        // Second login goes through the normal path.
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "s3e-alice", "password": test_password("t-s3e-legacy") })
                    .to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        set_legacy_password(None);
    }

    #[test]
    fn legacy_login_wrong_password_creates_nothing() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some(&test_password("t-s3e-legacy")));
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
        set_legacy_password(None);
    }

    #[test]
    fn legacy_login_invalid_id_is_400() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some(&test_password("t-s3e-legacy")));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "   ", "password": test_password("t-s3e-legacy") }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::BadRequest);
        set_legacy_password(None);
    }

    #[test]
    fn login_trims_padded_password() {
        let _g = lock();
        let client = setup();
        set_legacy_password(Some(&test_password("t-trim-legacy")));
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
        set_legacy_password(None);
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
