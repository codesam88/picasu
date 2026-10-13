use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::auth::invites::{self, InviteView};
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::put::users_password::caller_identity;
use crate::router::{AppResult, GuardResult};

/// Body for `POST /post/invites/create`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteRequest {
    /// Lifetime of the invite in hours, required: `0` means the invite
    /// never expires. Negative values are rejected. Pass 168 for the
    /// conventional 7-day lifetime.
    pub expires_in_hours: i64,
}

/// Response for `POST /post/invites/create`.
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
/// Corner cases: In open mode the ephemeral admin identity creates the
/// invite. Listing, expiry, use and revoke are handled by
/// `GET /get/invites` and `DELETE /delete/invites/{invite_id}`.
///
/// Errors: 400 negative `expiresInHours` — 401 missing or invalid admin
/// credentials — 405 read-only mode — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        request_body = CreateInviteRequest,
        responses(
            (status = 200, description = "Invite created; token returned once", body = CreateInviteResponse),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/invites/create", data = "<body>")]
pub fn create_invite_handler(
    auth: GuardResult<GuardAuth>,
    read_only: GuardResult<GuardReadOnlyMode>,
    body: Json<CreateInviteRequest>,
) -> AppResult<Json<CreateInviteResponse>> {
    let auth = auth?;
    let _ = read_only?;
    let (creator, _) = caller_identity(&auth.claims);
    let expires_at = invites::resolve_expiry(body.into_inner().expires_in_hours)?;
    let (token, record) = invites::create_invite_with_expiry(&creator, expires_at)?;
    Ok(Json(CreateInviteResponse {
        invite: record.into_view(invites::hash_token(&token)),
        token,
    }))
}
