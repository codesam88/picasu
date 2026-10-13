use rocket::post;
use rocket::serde::json::Json;
use serde::Serialize;

use crate::auth::invites::{self, InviteView};
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::put::users_password::caller_identity;
use crate::router::{AppResult, GuardResult};

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
/// `${origin}/register?invite=${token}`). Default expiry is 7 days; the
/// invite redeems once, for a single registration.
///
/// Corner cases: In open mode the ephemeral admin identity creates the
/// invite. Listing, expiry, use and revoke are handled by
/// `GET /get/invites` and `DELETE /delete/invites/{invite_id}`.
///
/// Errors: 401 missing or invalid admin credentials — 405 read-only mode —
/// 500 storage failure.
#[utoipa::path(
        tag = "auth",
        responses(
            (status = 200, description = "Invite created; token returned once", body = CreateInviteResponse),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/invites/create")]
pub fn create_invite_handler(
    auth: GuardResult<GuardAuth>,
    read_only: GuardResult<GuardReadOnlyMode>,
) -> AppResult<Json<CreateInviteResponse>> {
    let auth = auth?;
    let _ = read_only?;
    let (creator, _) = caller_identity(&auth.claims);
    let (token, record) = invites::create_invite(&creator)?;
    Ok(Json(CreateInviteResponse {
        invite: record.into_view(invites::hash_token(&token)),
        token,
    }))
}
