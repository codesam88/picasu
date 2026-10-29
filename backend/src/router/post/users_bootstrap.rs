use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tokio::task::spawn_blocking;

use crate::auth::users;
use crate::error::ResultExt;
use crate::openapi_components::Unauthorized;
use crate::router::auth::{GuardAdmin, GuardMutate, bootstrap_open};
use crate::router::post::users_register::UserSummary;
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// Body for `POST /post/users/bootstrap`. There is deliberately no `admin`
/// field: creation never grants admin through a body flag, and bootstrap
/// always forces `admin: true`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootstrapRequest {
    pub user_id: String,
    pub password: String,
}

/// Closed-window refusal, shared by the handler fast path and the
/// authoritative locked check in [`bootstrap_user_sync`] so contenders that
/// lose the race see the same message.
const WINDOW_CLOSED_MSG: &str = "No administrator enrolled: the bootstrap window is closed (user store is non-empty or --bootstrap-admin is off). Enroll further users via POST /post/users/register, or provision via the picasu-cmd CLI";

/// First-user enrollment lock: the window check and the insert must be
/// atomic across concurrent requests. The handler's pre-check stays a fast
/// path only; the check inside [`bootstrap_user_sync`] is authoritative.
static BOOTSTRAP_LOCK: Mutex<()> = Mutex::new(());

/// Enroll the first administrator and persist its password hash.
///
/// Thin delegate over [`users::provision_user`]: id/password validation
/// (including the default password policy for new passwords), duplicate
/// rejection, redb record insert, and password-store write all live there.
/// The trimmed (canonical) password is stored; login verifies the trimmed
/// input, so write-then-login stays consistent.
///
/// The flag ∧ empty-store window test runs here under [`BOOTSTRAP_LOCK`],
/// so concurrent first bootstraps serialize and exactly one wins; losers
/// get the closed-window 401.
///
/// # Errors
/// Returns 400 when the id is invalid or the password violates the
/// policy, 401 when the window is closed (flag off or store non-empty),
/// 409 when the id already exists, and 500 when either store fails.
fn bootstrap_user_sync(user_id: &str, password: &str) -> AppResult<UserSummary> {
    let _guard = BOOTSTRAP_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if crate::auth::user_store::count(&users::user_store_path()) > 0 || !bootstrap_open() {
        return Err(AppError::new(ErrorKind::Auth, WINDOW_CLOSED_MSG));
    }
    let id = users::provision_user(user_id, password, true)?;
    log::info!("enrollment: created user {id} (admin: true)");
    Ok(UserSummary {
        user_id: id,
        admin: true,
    })
}

/// Enroll the first administrator.
///
/// The only online admin-granting path. Open only while the user store is
/// empty **and `--bootstrap-admin` is set**; the created user is forced to
/// `admin: true`. Closed by default: with the flag off an empty store
/// rejects with the same 401 as every other guarded surface, and once any
/// user exists the window stays shut (further users come from
/// `POST /post/users/register`; role changes from `PUT /put/users/admin`).
///
/// Corner cases: The password is trimmed before hashing.
///
/// Errors: 400 invalid `userId` or password violating the policy — 401
/// the bootstrap window is closed (flag off, or the store is non-empty) —
/// 405 read-only mode — 409 `userId` already exists — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        request_body = BootstrapRequest,
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAdmin", "GuardMutate"],
            "credentials": ["session"],
            "admin": "admin",
            "shareScope": "n-a",
            "public": false,
            "effects": ["manage"],
            "invariants": ["AUTH-11", "AUTH-12"]
        }))),
        responses(
            (status = 200, description = "First administrator enrolled", body = UserSummary),
            (status = 400, description = "Invalid input"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 409, description = "User already exists"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/users/bootstrap", data = "<body>")]
pub async fn bootstrap_admin_handler(
    auth: GuardResult<GuardAdmin>,
    read_only: GuardResult<GuardMutate>,
    body: Json<BootstrapRequest>,
) -> AppResult<Json<UserSummary>> {
    let _ = read_only?;
    let non_empty = crate::auth::user_store::count(&users::user_store_path()) > 0;
    // The window is flag ∧ empty store. When it is shut, unauthenticated
    // callers see the guard's own closed-401; an authenticated caller that
    // reaches a shut window (valid session, non-empty store) is refused
    // with the same missing-enrollment 401.
    if non_empty || !bootstrap_open() {
        // Surface the guard's own closed-401 for unauthenticated callers; an
        // authenticated caller against a shut window falls through to the
        // missing-enrollment 401 below.
        auth?;
        return Err(AppError::new(ErrorKind::Auth, WINDOW_CLOSED_MSG));
    }
    let req = body.into_inner();
    let created = spawn_blocking(move || bootstrap_user_sync(&req.user_id, &req.password))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
    Ok(Json(created))
}
