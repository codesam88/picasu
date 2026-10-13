use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::auth::invites;
use crate::auth::users::{self, validate_new_password, validate_user_id};
use crate::error::ResultExt;
use crate::model::config::APP_CONFIG;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::post::users::UserSummary;
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// Body for `POST /post/users/register`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    pub user_id: String,
    pub password: String,
    /// Single-use invite token. Required while `allowRegistration` is off;
    /// ignored while it is on.
    pub invite: Option<String>,
}

/// Register a new user account.
///
/// Public route (no auth guard) behind the read-only guard. With
/// `allowRegistration` on, anyone registers without an invite; with it off,
/// a valid, unexpired, unused invite is required. The account is always
/// created with `admin: false` — `POST /post/users/create` on an empty
/// store remains the only online admin-granting path. While
/// `allowRegistration` is on, an `invite` in the body is ignored: open
/// registration supersedes it and does not burn the token.
///
/// Corner cases: The password is trimmed once and must satisfy the
/// default policy (see `auth::password::check_strength`); an empty `invite` string counts as absent. Invite redemption
/// marks the token used and inserts the user record in one transaction, so
/// a failed request never half-consumes an invite.
///
/// Errors: 400 invalid `userId` or password violating the policy — 403
/// registration disabled (no invite) or invite expired — 404 unknown or
/// revoked invite token — 405 read-only mode — 409 `userId` already exists
/// or invite already used — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        request_body = RegisterRequest,
        responses(
            (status = 200, description = "Account registered (non-admin)", body = UserSummary),
            (status = 400, description = "Invalid input"),
            (status = 403, description = "Registration disabled or invite expired"),
            (status = 404, description = "Unknown or revoked invite token"),
            (status = 405, description = "Read-only mode"),
            (status = 409, description = "User already exists or invite already used"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/users/register", data = "<body>")]
pub async fn register_user_handler(
    read_only: GuardResult<GuardReadOnlyMode>,
    body: Json<RegisterRequest>,
) -> AppResult<Json<UserSummary>> {
    let _ = read_only?;
    let allow_registration = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned")
        .allow_registration;
    let req = body.into_inner();
    // Password hashing is CPU-bound: the whole sync flow runs on the
    // blocking pool.
    spawn_blocking(move || register_user_sync(&req, allow_registration))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?
        .map(Json)
}

/// Validate the registration, create the non-admin user (directly, or by
/// redeeming an invite in the same transaction as the insert), then store
/// the password hash.
///
/// # Errors
/// 400 invalid id or weak password — 403 registration disabled or invite
/// expired — 404 unknown/revoked invite — 409 duplicate id or used invite —
/// 500 store failure. Runs inside `spawn_blocking` (KDF on the password).
fn register_user_sync(req: &RegisterRequest, allow_registration: bool) -> AppResult<UserSummary> {
    let id = validate_user_id(&req.user_id)?;
    let canonical = validate_new_password(&id, &req.password)?;
    if allow_registration {
        // Open registration: the invite is not consulted (open supersedes
        // bearer), so a stale link neither fails nor gets burned.
        if users::get_user(&id)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e))?
            .is_some()
        {
            return Err(AppError::new(
                ErrorKind::Conflict,
                format!("user already exists: {id}"),
            ));
        }
        users::create_user(&id, false).map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
    } else {
        let token = req
            .invite
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                AppError::new(
                    ErrorKind::PermissionDenied,
                    "registration is disabled; an invite is required",
                )
            })?;
        // Atomic: marks the invite used and inserts the user record in one
        // redb write transaction.
        invites::redeem_invite(&id, token)?;
    }
    users::store_password(&id, &canonical)?;
    log::info!("enrollment: registered user {id}");
    Ok(UserSummary {
        user_id: id,
        admin: false,
    })
}
