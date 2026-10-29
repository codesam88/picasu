use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::auth::invites;
use crate::auth::users::{self, validate_new_password, validate_user_id};
use crate::error::ResultExt;
use crate::openapi_components::Unauthorized;
use crate::router::auth::{GuardAdmin, GuardMutate};
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// Summary of a user record. Password hashes are never exported.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
    pub user_id: String,
    pub admin: bool,
}

/// Body for `POST /post/users/register`. There is deliberately no `admin`
/// field: registration always creates standard users; elevation is
/// `PUT /put/users/admin` only.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterRequest {
    pub user_id: String,
    pub password: String,
    /// Single-use invite token. Required while `allowRegistration` is off
    /// and the caller holds no admin session; ignored while it is on or
    /// the caller is an admin.
    pub invite: Option<String>,
}

/// Register a new user account.
///
/// Public route (no auth guard) behind the read-only guard. Three gates, in
/// order: a valid admin session registers without an invite; with
/// `allowRegistration` on, anyone registers without an invite; otherwise a
/// valid, unexpired, unused invite is required. Non-admin sessions count as
/// anonymous for gating. The account is always created with `admin: false` —
/// `POST /post/users/bootstrap` (flag ∧ empty store) is the only online
/// admin-granting path. While an invite is not needed (admin session or open
/// knob), an `invite` in the body is ignored: it neither fails nor gets
/// burned.
///
/// Corner cases: The password is trimmed once and must satisfy the
/// default policy (see `auth::password::check_strength`); an empty `invite` string counts as absent. Invite redemption
/// marks the token used and inserts the user record in one transaction, so
/// a failed request never half-consumes an invite. An invite may be bound
/// to a `username` at creation; redemption then requires that same id.
///
/// Errors: 400 invalid `userId` or password violating the policy — 403
/// registration disabled (no invite), invite expired, or invite username
/// mismatch — 404 unknown or revoked invite token — 405 read-only mode —
/// 409 `userId` already exists or invite already used — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        request_body = RegisterRequest,
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
            (status = 200, description = "Account registered (non-admin)", body = UserSummary),
            (status = 400, description = "Invalid input"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
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
    auth: GuardResult<GuardAdmin>,
    read_only: GuardResult<GuardMutate>,
    body: Json<RegisterRequest>,
) -> AppResult<Json<UserSummary>> {
    let _ = read_only?;
    // A valid admin session is a gate on its own. No session exists on an
    // empty store (the bootstrap window mints none), so anonymous callers
    // cannot squat the first-user slot as a non-admin. Non-admin sessions
    // fall through to the knob/invite gates.
    let admin_session = match &auth {
        Ok(guard) => {
            guard.claims.is_admin()
                && users::user_count().map_err(|e| AppError::from_err(ErrorKind::Database, e))? > 0
        }
        Err(_) => false,
    };
    let allow_registration = crate::model::config::with_config(|c| c.allow_registration);
    let req = body.into_inner();
    // Password hashing is CPU-bound: the whole sync flow runs on the
    // blocking pool.
    spawn_blocking(move || register_user_sync(&req, allow_registration, admin_session))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?
        .map(Json)
}

/// Validate the registration, create the non-admin user (via
/// `provision_user` on the gated path, or by redeeming an invite in the
/// same transaction as the insert), then store the password hash on the
/// invite path.
///
/// Refuses with 401 on an empty store (bootstrap first — registration never
/// creates the first user).
///
/// Gates in order: a valid admin session, open registration, or a valid
/// invite — otherwise 403/404. An admin session or the open knob bypasses
/// the invite entirely (a supplied token is ignored, never burned).
///
/// # Errors
/// 401 empty store (no administrator enrolled yet) — 400 invalid id or weak
/// password — 403 registration disabled or invite
/// expired — 404 unknown/revoked invite — 409 duplicate id or used invite —
/// 500 store failure. Runs inside `spawn_blocking` (KDF on the password).
fn register_user_sync(
    req: &RegisterRequest,
    allow_registration: bool,
    admin_session: bool,
) -> AppResult<UserSummary> {
    let id = validate_user_id(&req.user_id)?;
    let canonical = validate_new_password(&id, &req.password)?;
    // Registration never squats the first-user slot. On an empty store
    // the only online path is `POST /post/users/bootstrap`; refuse before
    // consulting the gates so neither the knob nor an invite can land the
    // first user as non-admin (an admin-less store recovers only via CLI).
    if crate::auth::user_store::count(&users::user_store_path()) == 0 {
        return Err(AppError::new(
            ErrorKind::Auth,
            crate::router::auth::not_enrolled_message(),
        ));
    }
    if admin_session || allow_registration {
        // Gated registration: the invite is not consulted (an admin session
        // or the open knob supersedes the bearer), so a stale link neither
        // fails nor gets burned. Delegates to `provision_user`, which
        // validates, inserts the user record, and stores the password hash.
        users::provision_user(&id, &canonical, false)?;
        log::info!("enrollment: registered user {id}");
        return Ok(UserSummary {
            user_id: id,
            admin: false,
        });
    }
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
    // Atomic per store, fixed order across stores: marks the invite used
    // and inserts the user record in one redb write transaction, then
    // writes the identity entry with its hash in one store rmw (see
    // `redeem_invite` for the crash-window semantics).
    invites::redeem_invite(&id, token, &canonical)?;
    log::info!("enrollment: registered user {id}");
    Ok(UserSummary {
        user_id: id,
        admin: false,
    })
}
