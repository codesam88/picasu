use rocket::Request;
use rocket::http::{Cookie, CookieJar, SameSite, Status};
use rocket::post;
use rocket::request::{FromRequest, Outcome};
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::auth::password::{PasswdFile, password_iterations, verify_password};
use crate::auth::users::{self, validate_user_id};
use crate::error::ResultExt;
use crate::openapi_components::Unauthorized;
use crate::router::auth::Claims;
use crate::router::{AppError, AppResult, ErrorKind};

use super::register::UserSummary;

/// Name of the session cookie. Read by the auth guards; written only by
/// the server via `Set-Cookie` (`HttpOnly` — see R5).
pub const SESSION_COOKIE_NAME: &str = "jwt";

/// Session lifetime in days. Matches the JWT `exp` minted in
/// [`Claims::new_user`]; the cookie is a carrier, expiry is enforced by
/// the signed claim, not by the cookie age.
const SESSION_MAX_AGE_DAYS: i64 = 14;

/// Whether the request arrived over TLS (via a forwarding proxy).
///
/// A request guard (handlers cannot take `&Request` directly) so the login
/// handler can set the `Secure` cookie flag conditionally. The backend
/// itself serves plain HTTP, so this is true only behind a TLS-terminating
/// proxy that forwards `X-Forwarded-Proto: https`. Plain-HTTP (default LAN)
/// deploys get a non-Secure cookie; proxies must forward the proto header
/// for `Secure` to engage.
pub struct ForwardedHttps(pub bool);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ForwardedHttps {
    type Error = std::convert::Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        Outcome::Success(Self(
            req.headers()
                .get_one("X-Forwarded-Proto")
                .is_some_and(|v| v.eq_ignore_ascii_case("https")),
        ))
    }
}

/// Issue the session cookie on `jar`: `HttpOnly` (never readable by page
/// JS — the R5 fix), `SameSite=Strict` (the CSRF defense), `Path=/`,
/// 14-day age, `Secure` iff the request came via forwarded HTTPS.
fn set_session_cookie(jar: &CookieJar<'_>, https: bool, token: String) {
    jar.add(
        Cookie::build((SESSION_COOKIE_NAME, token))
            .http_only(true)
            .secure(https)
            .same_site(SameSite::Strict)
            .path("/")
            .max_age(cookie::time::Duration::days(SESSION_MAX_AGE_DAYS))
            .build(),
    );
}

/// Request body for `POST /post/authenticate`: a user login object. There is
/// deliberately no `admin` field: unknown keys are rejected, never honored.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
/// mint a user-bound 14-day JWT plus the session profile. Must run on a
/// blocking thread: password hashing is CPU-heavy.
///
/// The password input is trimmed once and the trimmed (canonical) form is
/// verified; callers writing new passwords must store the trimmed form too
/// so write-then-login stays consistent.
///
/// # Errors
/// Returns 401 when the user is unknown or the password is wrong, and 500
/// when either store cannot be read.
fn login_sync(user_id: &str, password: &str) -> AppResult<(String, UserSummary)> {
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
    let token = Claims::new_user(user_id.to_string(), record.admin).encode();
    let profile = UserSummary {
        user_id: user_id.to_string(),
        admin: record.admin,
    };
    Ok((token, profile))
}

/// Sign in and issue the session cookie (`HttpOnly`).
///
/// A `{ userId, password }` object verifies against the user and password
/// stores. On success the JWT travels in a `Set-Cookie: jwt=...; HttpOnly;
/// SameSite=Strict; Path=/` response header — page JS never sees the token
/// (R5) — and the body carries only the session profile
/// `{ userId, admin }` for the client to display. Tokens are signed with
/// `authKey`, or with a secret generated once per process when `authKey`
/// is unset, so changing `authKey` invalidates every token issued under
/// the previous one.
///
/// Corner cases: While no users exist and `--bootstrap-admin` is set,
/// sign-in succeeds without credentials: every parseable login object is
/// accepted (open first-run mode), setting an ephemeral admin cookie and
/// returning the ephemeral profile without enrolling anybody.
///
/// Errors: 400 the login `userId` is empty or longer than 64 characters, or
/// the body does not parse as a login object — 401 unknown user or wrong
/// password.
#[utoipa::path(
        tag = "auth",
        request_body = LoginRequest,
        responses(
            (status = 200, description = "Session profile; JWT in HttpOnly Set-Cookie", body = UserSummary),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/authenticate", data = "<body>")]
pub async fn authenticate(
    jar: &CookieJar<'_>,
    https: ForwardedHttps,
    body: Json<LoginRequest>,
) -> AppResult<Json<UserSummary>> {
    // Open bootstrap mode: no users and the `--bootstrap-admin` flag set
    // accept any parseable login object without touching the stores.
    let open_mode = users::user_count().map_err(|e| AppError::from_err(ErrorKind::Database, e))?
        == 0
        && crate::router::auth::bootstrap_open();
    if open_mode {
        let token = Claims::new_user("admin".to_string(), true).encode();
        set_session_cookie(jar, https.0, token);
        return Ok(Json(UserSummary {
            user_id: "admin".to_string(),
            admin: true,
        }));
    }

    let login = body.into_inner();
    let user_id = validate_user_id(&login.user_id)?;
    let password = login.password;
    let (token, profile) = spawn_blocking(move || login_sync(&user_id, &password))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
    set_session_cookie(jar, https.0, token);
    Ok(Json(profile))
}

/// Clear the session cookie.
///
/// `HttpOnly` cookies cannot be removed by page JS, so logout is a server
/// round trip. Idempotent: succeeds with or without a live session; the
/// JWT itself stays cryptographically valid until expiry or `auth_key`
/// rotation (no server-side session store — see R6).
#[utoipa::path(
        tag = "auth",
        responses(
            (status = 204, description = "Session cookie cleared"),
        )
    )
]
#[post("/post/logout")]
pub fn logout(jar: &CookieJar<'_>) -> Status {
    jar.remove(Cookie::build(SESSION_COOKIE_NAME).path("/").build());
    Status::NoContent
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
        let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
        crate::router::auth::decode_typed(token, &validation).expect("decode token")
    }

    /// Extract the `jwt` value from the `Set-Cookie` response header.
    fn session_cookie_token(resp: &rocket::local::blocking::LocalResponse<'_>) -> String {
        let set_cookie = resp.headers().get_one("Set-Cookie").expect("Set-Cookie");
        let pair = set_cookie.split(';').next().expect("cookie pair");
        pair.strip_prefix("jwt=").expect("jwt cookie").to_string()
    }

    /// The session cookie must be HttpOnly (the R5 fix), SameSite=Strict,
    /// and path-scoped; `Secure` stays off on plain-HTTP test requests.
    fn assert_session_cookie_flags(resp: &rocket::local::blocking::LocalResponse<'_>) {
        let set_cookie = resp.headers().get_one("Set-Cookie").expect("Set-Cookie");
        assert!(
            set_cookie.contains("HttpOnly"),
            "cookie must be HttpOnly: {set_cookie}"
        );
        assert!(
            set_cookie.contains("SameSite=Strict"),
            "cookie must be SameSite=Strict: {set_cookie}"
        );
        assert!(
            set_cookie.contains("Path=/"),
            "cookie must be path-scoped: {set_cookie}"
        );
        assert!(
            !set_cookie.contains("Secure"),
            "plain-HTTP requests must not get Secure: {set_cookie}"
        );
    }

    #[test]
    fn authenticate_login_sets_httponly_cookie_and_returns_profile() {
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
        assert_session_cookie_flags(&resp);
        // The body is the session profile, never the raw token: a leaked
        // body (logs, devtools) must not yield a session.
        let body = resp.into_string().expect("profile body");
        assert!(!body.contains('.'), "body must not carry a JWT: {body}");
        let profile: serde_json::Value = serde_json::from_str(&body).expect("profile json");
        assert_eq!(profile["userId"], "s3c-alice");
        assert_eq!(profile["admin"], true);
    }

    #[test]
    fn authenticate_cookie_token_matches_profile() {
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
        let token = session_cookie_token(&resp);
        let body = resp.into_string().expect("profile body");
        let profile: serde_json::Value = serde_json::from_str(&body).expect("profile json");
        let claims = decode_claims(&token);
        match claims.role {
            crate::router::auth::Role::User { id, admin } => {
                assert_eq!(id, profile["userId"]);
                assert_eq!(admin, profile["admin"] == true);
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
    fn bare_string_body_is_rejected_not_migrated() {
        let _g = lock();
        let client = setup();
        set_bootstrap_admin(false);
        create_user_with_password("s3d-dave", false, &test_password("t-dave"));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!(test_password("t-s3d-pin")).to_string())
            .dispatch();
        // The legacy bare-string shape is not migrated: Rocket's `Json`
        // guard maps well-formed JSON of the wrong shape (string vs. login
        // object) to 422, and nothing is enrolled.
        assert_eq!(resp.status(), Status::UnprocessableEntity);
        assert_eq!(crate::auth::users::user_count().expect("count"), 1);
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
        assert_session_cookie_flags(&resp);
        let token = session_cookie_token(&resp);
        let body = resp.into_string().expect("profile body");
        let profile: serde_json::Value = serde_json::from_str(&body).expect("profile json");
        assert_eq!(profile["userId"], "admin");
        assert_eq!(profile["admin"], true);
        let claims = decode_claims(&token);
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
    fn object_with_unknown_admin_field_is_422() {
        let _g = lock();
        let client = setup();
        set_bootstrap_admin(false);
        create_user_with_password("s3d-erin", false, &test_password("t-erin"));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "s3d-erin", "password": test_password("t-erin"), "admin": true })
                    .to_string(),
            )
            .dispatch();
        // No `admin` field exists on any request body: the unknown key is
        // rejected, not honored or ignored.
        assert_eq!(resp.status(), Status::UnprocessableEntity);
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
    fn logout_clears_session_cookie_and_is_idempotent() {
        let _g = lock();
        let client = setup();
        create_user_with_password("s3c-zed", false, &test_password("t-zed"));
        let login = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "s3c-zed", "password": test_password("t-zed") }).to_string())
            .dispatch();
        assert_eq!(login.status(), Status::Ok);
        let token = session_cookie_token(&login);

        // Logout with the session cookie clears it …
        let resp = client
            .post("/post/logout")
            .cookie(rocket::http::Cookie::new("jwt", token))
            .dispatch();
        assert_eq!(resp.status(), Status::NoContent);
        let set_cookie = resp
            .headers()
            .get_one("Set-Cookie")
            .expect("clear Set-Cookie");
        assert!(
            set_cookie.starts_with("jwt=;"),
            "logout must expire jwt: {set_cookie}"
        );

        // … and stays 204 without any cookie (idempotent).
        let resp = client.post("/post/logout").dispatch();
        assert_eq!(resp.status(), Status::NoContent);
    }

    #[test]
    fn forwarded_https_proto_sets_secure_flag() {
        let _g = lock();
        let client = setup();
        create_user_with_password("s3c-sec", false, &test_password("t-sec"));
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .header(rocket::http::Header::new("X-Forwarded-Proto", "https"))
            .body(json!({ "userId": "s3c-sec", "password": test_password("t-sec") }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let set_cookie = resp.headers().get_one("Set-Cookie").expect("Set-Cookie");
        assert!(
            set_cookie.contains("Secure"),
            "proxied https must get Secure: {set_cookie}"
        );
        assert!(
            set_cookie.contains("HttpOnly"),
            "cookie must stay HttpOnly: {set_cookie}"
        );
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
