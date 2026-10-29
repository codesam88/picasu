use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::auth::invites::{self, InviteView};
use crate::auth::users;
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAdmin;
use crate::router::auth::GuardMutate;
use crate::router::put::users_password::caller_identity;
use crate::router::{AppResult, GuardResult};

/// Body for `POST /post/invites`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteRequest {
    /// Lifetime of the invite in hours, required: `0` means the invite
    /// never expires. Negative values are rejected. Pass 168 for the
    /// conventional 7-day lifetime.
    pub expires_in_hours: i64,
    /// Optional user id the invite is bound to. Trimmed; empty or
    /// missing means unbound. Validated as a user id.
    pub username: Option<String>,
}

/// Response for `POST /post/invites`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteResponse {
    /// Plaintext invite token — returned exactly once and never stored;
    /// it cannot be recovered after this response.
    pub token: String,
    /// The stored invite as it appears in listings (no token material).
    pub invite: InviteView,
}

/// Create a single-use invite token.
///
/// Admin-only. The `token` field is shown exactly once: only its blake3
/// hash is stored, so the value cannot be retrieved again — the admin
/// distributes it out of band (for example as
/// `${origin}/register?invite=${token}`). Expiry is required, in hours
/// (`0` never expires — pass 168 for the conventional 7-day lifetime);
/// the invite redeems once, for a single registration.
///
/// Corner cases: Creating an invite requires a live admin session, so on
/// an empty store invites are created only after the first bootstrap.
/// Listing, expiry, use and revoke are handled by
/// `GET /get/invites` and `DELETE /delete/invites/{invite_id}`. An optional
/// `username` binds the invite to that user id (403 on mismatch at redeem).
///
/// Errors: 400 negative `expiresInHours` or invalid `username` — 401 missing or invalid admin
/// credentials — 405 read-only mode — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        request_body = CreateInviteRequest,
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAdmin", "GuardMutate"],
            "tokenTyp": ["session"],
            "admin": "admin",
            "shareScope": "n-a",
            "public": false,
            "effects": [],
            "invariants": []
        }))),
        responses(
            (status = 200, description = "Invite created; token returned once", body = CreateInviteResponse),
            (status = 400, description = "Invalid input"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/invites", data = "<body>")]
pub fn create_invite_handler(
    auth: GuardResult<GuardAdmin>,
    read_only: GuardResult<GuardMutate>,
    body: Json<CreateInviteRequest>,
) -> AppResult<Json<CreateInviteResponse>> {
    let auth = auth?;
    let _ = read_only?;
    let (creator, _) = caller_identity(&auth.claims);
    let body = body.into_inner();
    let expires_at = invites::resolve_expiry(body.expires_in_hours)?;
    let username = body
        .username
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    // Validate as a user id (400 on invalid) before storing.
    let username = username.map(users::validate_user_id).transpose()?;
    let (token, record) =
        invites::create_invite_with_expiry(&creator, expires_at, username.as_deref())?;
    Ok(Json(CreateInviteResponse {
        invite: record.into_view(invites::hash_token(&token)),
        token,
    }))
}
