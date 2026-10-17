use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use rocket::Request;
use rocket::http::{Cookie, CookieJar, SameSite, Status};
use rocket::post;
use rocket::request::{FromRequest, Outcome};
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::auth::login_throttle::{LoginThrottle, select_client_ip};
use crate::auth::password::{PasswdFile, password_iterations, verify_password};
use crate::auth::users::{self, validate_user_id};
use crate::error::ResultExt;
use crate::model::config::APP_CONFIG;
use crate::openapi_components::Unauthorized;
use crate::router::auth::{Claims, Role, VALIDATION, decode_typed};
use crate::router::{AppError, AppResult, ErrorKind};

use super::register::UserSummary;

/// Name of the session cookie. Read by the auth guards; written only by
/// the server via `Set-Cookie` (`HttpOnly`).
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
/// JS), `SameSite=Strict` (the CSRF defense), `Path=/`,
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

/// Name of the environment-only kill switch for the login throttle
/// (schemathesis fuzzing). Read straight from the process environment at
/// request time — never from `[server]` config and never API-writable —
/// mirroring `PICASU_PBKDF_ITERATIONS` in `auth/password.rs`.
pub(crate) const RATE_LIMITS_ENV_VAR: &str = "PICASU_RATE_LIMITS";

/// The three login-throttle bucket schedules read from `APP_CONFIG`
/// (IP-short, IP-long, user-long). Kept beside the tracker so a parameter
/// change rebuilds it: the knobs are host-only, so in production this
/// happens only at boot; tests mutate `APP_CONFIG` directly and the
/// rebuild honors that.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ThrottleParams {
    ip_short: (u64, u32, u64, u64),
    ip_long: (u64, u32, u64, u64),
    user_long: (u64, u32, u64, u64),
}

fn throttle_params() -> ThrottleParams {
    let config = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("APP_CONFIG lock");
    ThrottleParams {
        ip_short: (
            config.login_ip_window_secs,
            config.login_ip_max_failures,
            config.login_ip_base_ban_secs,
            config.login_ip_max_ban_secs,
        ),
        ip_long: (
            config.login_ip_long_window_secs,
            config.login_ip_long_max_failures,
            config.login_ip_long_base_ban_secs,
            config.login_ip_long_max_ban_secs,
        ),
        user_long: (
            config.login_user_long_window_secs,
            config.login_user_long_max_failures,
            config.login_user_long_base_ban_secs,
            config.login_user_long_max_ban_secs,
        ),
    }
}

/// Process-wide login throttle tracker, built lazily from the current
/// config and rebuilt when the schedules change. A plain [`Mutex`]: the
/// critical sections are microseconds of in-memory `HashMap` work behind
/// one route, so a sharded/lock-free table would add complexity without a
/// contention win.
static LOGIN_THROTTLE: Mutex<Option<(ThrottleParams, LoginThrottle)>> = Mutex::new(None);

/// Timestamp of the last reaping pass, for the at-most-once-per-60s
/// opportunistic [`LoginThrottle::sweep`] on the login path.
static LAST_SWEEP: AtomicU64 = AtomicU64::new(0);

/// Minimum seconds between opportunistic sweeps.
const SWEEP_INTERVAL_SECS: u64 = 60;

/// Whether the environment-only throttle kill switch is active: only the
/// exact string `false` disables throttling. Unset or any other value
/// (including `FALSE` or `false `) leaves it enabled — a fuzz harness sets
/// it deliberately, while an operator typo must never silently disable
/// brute-force protection. Read from the process environment at request
/// time, mirroring `PICASU_PBKDF_ITERATIONS`.
fn rate_limits_disabled() -> bool {
    matches!(std::env::var(RATE_LIMITS_ENV_VAR), Ok(value) if value == "false")
}

/// Warn once per process when the kill switch is active, so an operator
/// notices that brute-force protection is off.
static RATE_LIMITS_OFF_WARNED: std::sync::Once = std::sync::Once::new();

/// Emission counts for the two throttle warn sites. Tests read these
/// instead of capturing log records: the process-global logger slot is
/// claimed by whichever component installs first (Rocket's ignite and the
/// parity checker both install one), so record capture races across
/// parallel tests.
#[cfg(test)]
static LOGIN_FAILED_WARNS: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
static RATE_LIMITS_OFF_WARNS: AtomicU64 = AtomicU64::new(0);

/// Run `f` against the shared tracker, (re)building it first when the
/// configured schedules differ from the ones it was built with.
fn with_login_throttle<R>(f: impl FnOnce(&mut LoginThrottle) -> R) -> R {
    let params = throttle_params();
    let mut slot = LOGIN_THROTTLE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if slot.as_ref().is_none_or(|(built, _)| *built != params) {
        let tracker = LoginThrottle::new(params.ip_short, params.ip_long, params.user_long)
            .expect("login throttle schedules are validated at config load");
        *slot = Some((params, tracker));
    }
    let (_, tracker) = slot.as_mut().expect("tracker installed above");
    f(tracker)
}

/// Drop the shared tracker (test setup; production keeps one tracker for
/// the process lifetime and relies on restart-clear).
#[cfg(test)]
pub(crate) fn reset_login_throttle() {
    *LOGIN_THROTTLE
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = None;
    LAST_SWEEP.store(0, Ordering::Relaxed);
}

/// Client-IP key for the login throttle, resolved once per request like
/// [`ForwardedHttps`] (route handlers cannot take `&Request` directly).
///
/// The socket address comes from the transport layer (`Request::remote`),
/// not `Request::client_ip`: with Rocket's default `ip_header`
/// (`X-Real-IP`), `client_ip()` trusts a client-supplied header, which
/// would let callers rotate identities past the IP buckets. The
/// forwarded-for header is consulted only when the operator enables
/// `trust_proxy_forwarded_for`.
pub struct ThrottleClientIp(pub String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ThrottleClientIp {
    type Error = std::convert::Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let trust_proxy = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("APP_CONFIG lock")
            .trust_proxy_forwarded_for;
        Outcome::Success(Self(select_client_ip(
            req.headers().get_one("X-Forwarded-For"),
            req.remote().map(|addr| addr.ip().to_string()).as_deref(),
            trust_proxy,
        )))
    }
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
    let claims = Claims::new_user(user_id.to_string(), record.admin);
    users::add_session(user_id, &claims.jti)?;
    let token = claims.encode();
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
/// — and the body carries only the session profile
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
/// password — 429 the attempt is throttled (`Retry-After` carries the
/// remaining ban seconds) — 500 internal error.
///
/// Brute-force throttle: before the KDF runs, every non-open attempt is
/// checked against the IP-short, IP-long, and user-long buckets (see
/// [`crate::auth::rate_limit`]); while any bucket bans, a 429 is returned
/// without hashing. Evaluated failures record into all applicable buckets,
/// a success clears the caller's entries, and throttled attempts are never
/// recorded. Open bootstrap mode (no users + flag) skips the throttle:
/// there is no credential to guess. The environment-only kill switch
/// `PICASU_RATE_LIMITS=false` (exact match) also bypasses the throttle —
/// for fuzzing runs — while the failure logging stays.
#[utoipa::path(
        tag = "auth",
        request_body = LoginRequest,
        responses(
            (status = 200, description = "Session profile; JWT in HttpOnly Set-Cookie", body = UserSummary),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 429, description = "Too many login attempts; see Retry-After"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/authenticate", data = "<body>")]
pub async fn authenticate(
    jar: &CookieJar<'_>,
    https: ForwardedHttps,
    ip: ThrottleClientIp,
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

    // Env-only kill switch (`PICASU_RATE_LIMITS=false`, exact match): the
    // throttle is bypassed entirely — no check, no recording on either
    // outcome — while the failure logging below stays. `now = None` is the
    // single "no throttle contact" signal for the rest of this handler.
    let now = if rate_limits_disabled() {
        RATE_LIMITS_OFF_WARNED.call_once(|| {
            #[cfg(test)]
            RATE_LIMITS_OFF_WARNS.fetch_add(1, Ordering::Relaxed);
            log::warn!("{RATE_LIMITS_ENV_VAR}=false: login brute-force throttling is DISABLED");
        });
        None
    } else {
        // Fail open on a pre-epoch clock: a wrong `now` would corrupt every
        // countdown (ban ends computed off a wrong epoch last for years), so
        // the attempt proceeds unthrottled and the clock error is logged.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .map_err(|e| log::warn!("login throttle skipped for this attempt (clock error): {e}"))
            .ok()
    };

    if let Some(now) = now {
        let ban = with_login_throttle(|throttle| {
            // Opportunistic reaping at most once a minute; the tracker's
            // entry caps bound memory between sweeps.
            if now.saturating_sub(LAST_SWEEP.load(Ordering::Relaxed)) >= SWEEP_INTERVAL_SECS {
                throttle.sweep(now);
                LAST_SWEEP.store(now, Ordering::Relaxed);
            }
            throttle.check(&ip.0, &user_id, now)
        });
        if ban > 0 {
            log::warn!("login throttled: ip={} user_id={}", ip.0, user_id);
            return Err(AppError::new(
                ErrorKind::RateLimited,
                format!("Too many login attempts, try again in {ban}s"),
            )
            .retry_after(ban));
        }
    }

    let password = login.password;
    let login_user = user_id.clone();
    let (token, profile) = match spawn_blocking(move || login_sync(&login_user, &password))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?
    {
        Ok(ok) => ok,
        Err(err) => {
            if err.kind == ErrorKind::Auth {
                // Throttled attempts are logged too, but only evaluated
                // failures (401s) are recorded; 400s never reach here.
                #[cfg(test)]
                LOGIN_FAILED_WARNS.fetch_add(1, Ordering::Relaxed);
                log::warn!("login failed: ip={} user_id={}", ip.0, user_id);
                if let Some(now) = now {
                    with_login_throttle(|throttle| {
                        throttle.record_failure(&ip.0, &user_id, now);
                    });
                }
            }
            return Err(err);
        }
    };
    // When `now` is None (pre-epoch clock or the `PICASU_RATE_LIMITS`
    // kill switch) the tracker was never consulted, so there is nothing
    // to clear; otherwise success drops the caller's entries.
    if now.is_some() {
        with_login_throttle(|throttle| throttle.record_success(&ip.0, &user_id));
    }
    set_session_cookie(jar, https.0, token);
    Ok(Json(profile))
}

/// Clear the session cookie.
///
/// `HttpOnly` cookies cannot be removed by page JS, so logout is a server
/// round trip. Idempotent: succeeds with or without a live session; the
/// JWT itself stays cryptographically valid until expiry or `auth_key`
/// rotation (server-side sessions revoke on logout and password change).
#[utoipa::path(
        tag = "auth",
        responses(
            (status = 204, description = "Session cookie cleared"),
        )
    )
]
#[post("/post/logout")]
pub fn logout(jar: &CookieJar<'_>) -> Status {
    // Revoke only the presented session (logout is local): a missing,
    // undecodable, or expired token simply skips removal. The cookie is
    // always cleared and the answer is always 204.
    let presented = jar
        .get(SESSION_COOKIE_NAME)
        .map(|cookie| cookie.value().to_string())
        .and_then(|token| decode_typed::<Claims>(&token, &VALIDATION).ok());
    if let Some(claims) = presented
        && let Role::User { id, .. } = &claims.role
    {
        let _ = users::remove_session(id, &claims.jti);
    }
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

    /// The session cookie must be HttpOnly, SameSite=Strict,
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

    fn post_login<'c>(
        client: &'c rocket::local::blocking::Client,
        user_id: &str,
        password: &str,
    ) -> rocket::local::blocking::LocalResponse<'c> {
        client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": user_id, "password": password }).to_string())
            .dispatch()
    }

    /// The IP-short schedule (3 failures / 600s → 10s ban) as shipped in
    /// `crate::auth::rate_limit`: three evaluated failures stay under the
    /// threshold, the fourth evaluated failure opens the 10s ban and still
    /// answers 401 (no same-request escalation), so the attempt *after*
    /// the ban opens is the first 429.
    #[test]
    fn authenticate_throttled_login_is_429_with_retry_after() {
        let _g = lock();
        let client = setup();
        create_user_with_password("t7-alice", false, &test_password("t7-pw"));
        let wrong = test_password("t7-wrong");
        for _ in 0..4 {
            let resp = post_login(&client, "t7-alice", &wrong);
            assert_eq!(resp.status(), Status::Unauthorized);
        }
        let resp = post_login(&client, "t7-alice", &wrong);
        assert_eq!(resp.status(), Status::TooManyRequests);
        assert_eq!(resp.headers().get_one("Retry-After"), Some("10"));
        let body = resp.into_string().expect("429 body");
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("429 json");
        assert_eq!(parsed["kind"], "Rate Limited");
        assert_eq!(parsed["retryAfter"], 10);
        assert!(
            parsed["message"]
                .as_str()
                .expect("message string")
                .contains("try again in 10s"),
            "human message must carry the countdown: {body}"
        );
    }

    #[test]
    fn authenticate_throttled_attempt_skips_kdf() {
        let _g = lock();
        let client = setup();
        create_user_with_password("t7-fast", false, &test_password("t7-fast-pw"));
        let wrong = test_password("t7-fast-wrong");
        for _ in 0..4 {
            assert_eq!(
                post_login(&client, "t7-fast", &wrong).status(),
                Status::Unauthorized
            );
        }
        // Reference: one KDF round at the effort currently in force.
        let start = std::time::Instant::now();
        let _ = super::run_dummy_verify();
        let kdf_round = start.elapsed();

        let start = std::time::Instant::now();
        let resp = post_login(&client, "t7-fast", &wrong);
        let throttled = start.elapsed();
        assert_eq!(resp.status(), Status::TooManyRequests);
        // Flakiness guard: at production effort a KDF round takes tens of
        // milliseconds or more, so "well under one round" holds with a 5x
        // margin. Under the test-only iteration override (1000) a round
        // collapses below HTTP-dispatch noise, where only a generous
        // absolute bound is meaningful; the check-before-KDF order itself
        // is pinned structurally by the handler.
        if kdf_round.as_millis() >= 20 {
            assert!(
                throttled.as_micros() * 5 < kdf_round.as_micros(),
                "throttled answer ({throttled:?}) must cost well under one KDF round ({kdf_round:?})"
            );
        } else {
            assert!(
                throttled < std::time::Duration::from_millis(250),
                "throttled answer ({throttled:?}) must stay cheap"
            );
        }
    }

    #[test]
    fn authenticate_success_resets_throttle_history() {
        let _g = lock();
        let client = setup();
        create_user_with_password("t7-reset", false, &test_password("t7-reset-pw"));
        let wrong = test_password("t7-reset-wrong");
        for _ in 0..3 {
            assert_eq!(
                post_login(&client, "t7-reset", &wrong).status(),
                Status::Unauthorized
            );
        }
        let resp = post_login(&client, "t7-reset", &test_password("t7-reset-pw"));
        assert_eq!(resp.status(), Status::Ok, "success clears the counters");
        // Same-count failures again: without the success reset the second
        // of these would already be a 429 (the pre-success failures would
        // still count toward the ban).
        for _ in 0..4 {
            assert_eq!(
                post_login(&client, "t7-reset", &wrong).status(),
                Status::Unauthorized
            );
        }
        let resp = post_login(&client, "t7-reset", &wrong);
        assert_eq!(
            resp.status(),
            Status::TooManyRequests,
            "fresh failures must trip the ban again"
        );
        assert_eq!(resp.headers().get_one("Retry-After"), Some("10"));
    }

    #[test]
    fn authenticate_unknown_user_failures_count_toward_ip_throttle() {
        let _g = lock();
        let client = setup();
        set_bootstrap_admin(false);
        let wrong = test_password("t7-ghost-wrong");
        // An unknown name answers exactly like a known one (401 + a KDF
        // round, no oracle) and burns the same IP budget.
        for _ in 0..4 {
            let resp = post_login(&client, "t7-ghost", &wrong);
            assert_eq!(resp.status(), Status::Unauthorized);
        }
        let resp = post_login(&client, "t7-ghost", &wrong);
        assert_eq!(
            resp.status(),
            Status::TooManyRequests,
            "unknown-name failures must trip the IP bucket identically"
        );
        assert_eq!(resp.headers().get_one("Retry-After"), Some("10"));
        assert_eq!(
            crate::auth::users::user_count().expect("count"),
            0,
            "no user may be created by the hammering"
        );
    }

    #[test]
    fn authenticate_invalid_user_id_never_trips_throttle() {
        let _g = lock();
        let client = setup();
        create_user_with_password("t7-id", false, &test_password("t7-id-pw"));
        // More 400s than the bucket threshold: they never enter it.
        for _ in 0..6 {
            let resp = post_login(&client, "   ", &test_password("t7-id-wrong"));
            assert_eq!(resp.status(), Status::BadRequest);
        }
        let resp = post_login(&client, "t7-id", &test_password("t7-id-wrong"));
        assert_eq!(
            resp.status(),
            Status::Unauthorized,
            "a valid login must be unaffected by preceding 400s"
        );
        let resp = post_login(&client, "t7-id", &test_password("t7-id-pw"));
        assert_eq!(resp.status(), Status::Ok);
    }

    #[test]
    fn authenticate_open_mode_skips_throttle() {
        let _g = lock();
        let client = setup();
        set_bootstrap_admin(true);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        // Open bootstrap has no credential to brute-force (every parseable
        // login succeeds), so repeated logins stay 200 and unthrottled.
        for i in 0..6 {
            let resp = post_login(&client, &format!("t7-open-{i}"), &test_password("t7-open"));
            assert_eq!(resp.status(), Status::Ok, "attempt {i}");
        }
    }

    /// Set (or clear) `PICASU_RATE_LIMITS` for one test, restoring the
    /// previous value afterwards. Env mutation is `unsafe` in edition
    /// 2024; the crate-level deny is lifted for this test-only helper,
    /// which is safe because the caller holds `TEST_SERIAL_GUARD` and the
    /// only other readers of this variable are authenticate dispatches,
    /// serialized by the same guard.
    #[allow(unsafe_code)]
    fn with_rate_limits_env(value: Option<&str>, f: impl FnOnce()) {
        let saved = std::env::var(super::RATE_LIMITS_ENV_VAR).ok();
        struct Restore(Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: see the helper's doc; runs before the guard is
                // released.
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var(super::RATE_LIMITS_ENV_VAR, v),
                        None => std::env::remove_var(super::RATE_LIMITS_ENV_VAR),
                    }
                }
            }
        }
        let _restore = Restore(saved);
        // SAFETY: see the helper's doc.
        unsafe {
            match value {
                Some(v) => std::env::set_var(super::RATE_LIMITS_ENV_VAR, v),
                None => std::env::remove_var(super::RATE_LIMITS_ENV_VAR),
            }
        }
        f();
    }

    /// `PICASU_RATE_LIMITS=false` is the fuzzing kill switch: no check, no
    /// recording, no 429 — hammering stays at plain 401s.
    #[test]
    fn rate_limits_env_false_disables_throttle() {
        let _g = lock();
        with_rate_limits_env(Some("false"), || {
            let client = setup();
            create_user_with_password("t8-off", false, &test_password("t8-off-pw"));
            let wrong = test_password("t8-off-wrong");
            // One more failure than the IP-short bucket needs to trip.
            for _ in 0..7 {
                let resp = post_login(&client, "t8-off", &wrong);
                assert_eq!(resp.status(), Status::Unauthorized);
                assert!(resp.headers().get_one("Retry-After").is_none());
            }
            // Without the switch the same 7 attempts end in a 429; a
            // correct login must still work after the hammering.
            let resp = post_login(&client, "t8-off", &test_password("t8-off-pw"));
            assert_eq!(resp.status(), Status::Ok);
        });
    }

    /// Unset, `true`, or garbage: anything but the exact string `false`
    /// leaves the throttle enabled.
    #[test]
    fn rate_limits_env_values_other_than_false_keep_throttle() {
        let trip = |client: &rocket::local::blocking::Client| {
            create_user_with_password("t8-on", false, &test_password("t8-on-pw"));
            let wrong = test_password("t8-on-wrong");
            for _ in 0..4 {
                assert_eq!(
                    post_login(client, "t8-on", &wrong).status(),
                    Status::Unauthorized
                );
            }
            assert_eq!(
                post_login(client, "t8-on", &wrong).status(),
                Status::TooManyRequests,
                "throttle must stay active"
            );
        };
        let _g = lock();
        with_rate_limits_env(None, || {
            let client = setup();
            trip(&client);
        });
        with_rate_limits_env(Some("true"), || {
            let client = setup();
            trip(&client);
        });
        with_rate_limits_env(Some("False"), || {
            let client = setup();
            trip(&client);
        });
        with_rate_limits_env(Some("garbage"), || {
            let client = setup();
            trip(&client);
        });
    }

    /// While the kill switch is on, evaluated failures keep emitting the
    /// `ip=` / `user_id=` warn (only the throttle is skipped). Emission is
    /// observed through `#[cfg(test)]` counters at the warn sites: the
    /// process-global logger slot is claimed by whichever test gets there
    /// first (Rocket's ignite and the parity checker install loggers too),
    /// so record capture would race.
    #[test]
    fn rate_limits_disabled_failure_logging_stays() {
        let _g = lock();
        with_rate_limits_env(Some("false"), || {
            let client = setup();
            create_user_with_password("t8-log", false, &test_password("t8-log-pw"));
            let before = super::LOGIN_FAILED_WARNS.load(std::sync::atomic::Ordering::Relaxed);
            let off_before =
                super::RATE_LIMITS_OFF_WARNS.load(std::sync::atomic::Ordering::Relaxed);
            let resp = post_login(&client, "t8-log", &test_password("t8-log-wrong"));
            assert_eq!(resp.status(), Status::Unauthorized);
            assert_eq!(
                super::LOGIN_FAILED_WARNS.load(std::sync::atomic::Ordering::Relaxed),
                before + 1,
                "the ip=/user_id= failure warn must still fire while throttling is disabled"
            );
            // Best-effort: the Once may already have been consumed by an
            // earlier test in this process, so only the upper bound holds.
            assert!(
                super::RATE_LIMITS_OFF_WARNS.load(std::sync::atomic::Ordering::Relaxed)
                    - off_before
                    <= 1,
                "the disabled warning fires at most once per process"
            );
        });
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

    #[test]
    fn live_login_cookie_jti_is_accepted_and_forged_is_401_and_open_mode_works() {
        let _g = lock();
        let client = setup();
        create_user_with_password("s3c-live", true, &test_password("t-live"));
        // Live login mints and stores jti; the cookie must be accepted by a guarded route.
        let resp = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(json!({ "userId": "s3c-live", "password": test_password("t-live") }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let token = session_cookie_token(&resp);
        let claims = decode_claims(&token);
        // Session store must contain the jti.
        let record = crate::auth::users::get_user("s3c-live")
            .expect("get user")
            .expect("record");
        assert!(record.sessions.iter().any(|s| s.jti == claims.jti));

        let resp = client
            .get("/get/index/status")
            .cookie(rocket::http::Cookie::new("jwt", token.clone()))
            .dispatch();
        assert_eq!(
            resp.status(),
            Status::Ok,
            "live login cookie must be accepted"
        );

        // Forged/unknown jti on same user must be 401.
        let forged = crate::router::auth::Claims::new_user("s3c-live".to_string(), true).encode();
        let resp = client
            .get("/get/index/status")
            .cookie(rocket::http::Cookie::new("jwt", forged))
            .dispatch();
        assert_eq!(
            resp.status(),
            Status::Unauthorized,
            "forged jti must be 401"
        );

        // Legacy-shaped payload without jti must fail decode.
        {
            let key = crate::model::config::APP_CONFIG
                .get()
                .expect("APP_CONFIG not initialized")
                .read()
                .expect("lock poisoned")
                .get_jwt_secret_key();
            #[derive(serde::Serialize)]
            #[serde(rename_all = "camelCase")]
            struct LegacyNoJti {
                role: crate::router::auth::Role,
                exp: u64,
                #[serde(rename = "typ")]
                typ: crate::router::auth::TokenType,
            }
            let legacy = LegacyNoJti {
                role: crate::router::auth::Role::User {
                    id: "s3c-live".to_string(),
                    admin: true,
                },
                exp: 9_999_999_999,
                typ: crate::router::auth::TokenType::Admin,
            };
            let legacy_token = jsonwebtoken::encode(
                &jsonwebtoken::Header::default(),
                &legacy,
                &jsonwebtoken::EncodingKey::from_secret(&key),
            )
            .expect("encode legacy");
            let res: anyhow::Result<crate::router::auth::Claims> =
                crate::router::auth::decode_typed(
                    &legacy_token,
                    &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256),
                );
            assert!(res.is_err(), "legacy without jti must fail");
        }

        // Open-mode ephemeral token still works with no record.
        crate::tests::bootstrap::reset_backend_state();
        let client2 = setup();
        // Store is empty, bootstrap flag is true by default after reset.
        let resp = client2.get("/get/index/status").dispatch();
        assert_eq!(
            resp.status(),
            Status::Ok,
            "open-mode without cookie must still be 200"
        );
        // Also an ephemeral token minted without a record should be irrelevant — open-mode
        // bypasses session check, but we verify the unauthenticated open path still succeeds.
    }
}
