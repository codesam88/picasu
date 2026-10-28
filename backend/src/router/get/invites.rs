use rocket::get;
use rocket::serde::json::Json;

use crate::auth::invites::{self, InviteView};
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::router::{AppResult, GuardResult};

/// List invite tokens' metadata.
///
/// Admin-only. Returns every invite with its display `prefix`, creator,
/// expiry and — once redeemed — the `usedBy` user id. Tokens themselves are
/// never returned: `id` is the blake3 hash of the token (preimage-resistant
/// and useless as a bearer) and identifies the invite for
/// `DELETE /delete/invites/{invite_id}`.
///
/// Corner cases: A store that has never seen an invite returns an empty
/// list rather than an error.
///
/// Errors: 401 missing or invalid admin credentials — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAuth"],
            "tokenTyp": ["admin"],
            "admin": "admin",
            "shareScope": "n-a",
            "readonly": false,
            "reauth": false,
            "public": false,
            "effects": [],
            "invariants": []
        }))),
        responses(
            (status = 200, description = "Invite metadata, never tokens", body = [InviteView]),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/get/invites")]
pub fn list_invites_handler(auth: GuardResult<GuardAuth>) -> AppResult<Json<Vec<InviteView>>> {
    let _ = auth?;
    Ok(Json(invites::list_invites()?))
}
