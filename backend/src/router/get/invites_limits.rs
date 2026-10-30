use rocket::get;
use rocket::serde::json::Json;
use serde::Serialize;

use crate::auth::invites::MAX_EXPIRY_HOURS;
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAdmin;
use crate::router::{AppResult, GuardResult};

/// Invite creation limits for the admin console.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InviteLimits {
    /// Longest `expiresInHours` `POST /post/invites` accepts (one year).
    /// Larger values are 400; `0` still means never expires.
    pub max_expiry_hours: i64,
}

/// Report the invite creation limits the admin console enforces.
///
/// Admin-only. The console fetches this when building the invite panel so
/// the client-side expiry parser validates against the same bound the
/// server enforces, instead of hard-coding policy.
///
/// Errors: 401 missing or invalid admin credentials.
#[utoipa::path(
        tag = "auth",
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAdmin"],
            "credentials": ["session"],
            "admin": "admin",
            "shareScope": "n-a",
            "effects": ["read"],
            "invariants": ["AUTH-11"]
        }))),
        responses(
            (status = 200, description = "Invite creation limits", body = InviteLimits),
            (status = 401, response = Unauthorized),
        )
    )
]
#[get("/get/invites/limits")]
pub fn invite_limits_handler(auth: GuardResult<GuardAdmin>) -> AppResult<Json<InviteLimits>> {
    let _ = auth?;
    Ok(Json(InviteLimits {
        max_expiry_hours: MAX_EXPIRY_HOURS,
    }))
}

#[cfg(test)]
mod tests {
    use rocket::http::Status;

    use crate::tests::bootstrap::{cookie, lock, seed, setup};

    #[test]
    fn limits_reports_max_expiry_hours() {
        let _g = lock();
        let client = setup();
        seed("s6-root", true, None);
        let resp = client
            .get("/get/invites/limits")
            .cookie(cookie("s6-root", true))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("body");
        assert_eq!(
            body,
            format!(
                "{{\"maxExpiryHours\":{}}}",
                crate::auth::invites::MAX_EXPIRY_HOURS
            )
        );
    }

    #[test]
    fn limits_non_admin_is_401() {
        let _g = lock();
        let client = setup();
        seed("s6-root", true, None);
        seed("s6-bob", false, None);
        let status = client
            .get("/get/invites/limits")
            .cookie(cookie("s6-bob", false))
            .dispatch()
            .status();
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn limits_unauthenticated_is_401() {
        let _g = lock();
        let client = setup();
        seed("s6-root", true, None);
        let status = client.get("/get/invites/limits").dispatch().status();
        assert_eq!(status, Status::Unauthorized);
    }
}
