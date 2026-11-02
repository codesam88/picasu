use rocket::http::Status;
use rocket::put;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::auth::users::{self, validate_user_id};
use crate::openapi_components::Unauthorized;
use crate::router::auth::{GuardAdmin, GuardMutate, Role};
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// Body for `PUT /put/users/sharing`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetUserSharingRequest {
    pub user_id: String,
    pub enabled: bool,
}

/// Grant or revoke the sharing flag for `target`.
fn set_sharing_sync(actor_id: &str, target_user_id: &str, enabled: bool) -> AppResult<bool> {
    let target = validate_user_id(target_user_id)?;
    let changed = users::set_sharing_enabled_role(&target, enabled)?;
    if changed {
        log::info!("account sharing change: actor={actor_id} target={target} enabled={enabled}");
    }
    Ok(changed)
}

/// Set a user's sharing flag.
///
/// Admin-only. Sharing is not a liveness property, so no zero-enabled-admin
/// check applies. A same-flag write is a no-op reporting success without an
/// audit line.
///
/// Errors: 400 invalid `userId` — 401 missing or invalid credentials,
/// non-admin caller, or share credentials — 404 unknown `userId` — 405
/// read-only mode — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        request_body = SetUserSharingRequest,
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAdmin", "GuardMutate"],
            "credentials": ["session"],
            "admin": "admin",
            "shareScope": "n-a",
            "effects": ["manage"],
            "invariants": ["AUTH-11", "AUTH-12"]
        }))),
        responses(
            (status = 200, description = "Account sharing state updated"),
            (status = 400, description = "Invalid input"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 404, description = "User not found"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/users/sharing", data = "<body>")]
pub fn set_user_sharing_handler(
    auth: GuardResult<GuardAdmin>,
    read_only: GuardResult<GuardMutate>,
    body: Json<SetUserSharingRequest>,
) -> AppResult<Status> {
    let _ = read_only?;
    let auth = auth?;
    let GuardAdmin { claims } = auth;
    let Role::User { id: actor_id, .. } = claims.role else {
        return Err(AppError::new(ErrorKind::Auth, "Share token not accepted"));
    };
    let req = body.into_inner();
    set_sharing_sync(&actor_id, &req.user_id, req.enabled)?;
    Ok(Status::Ok)
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Cookie, Status};
    use serde_json::json;

    use crate::tests::bootstrap::{cookie, lock, seed, setup};

    fn put_sharing(
        client: &rocket::local::blocking::Client,
        cookie: Option<Cookie<'static>>,
        body: &serde_json::Value,
    ) -> Status {
        let mut req = client
            .put("/put/users/sharing")
            .header(ContentType::JSON)
            .body(body.to_string());
        if let Some(c) = cookie {
            req = req.cookie(c);
        }
        req.dispatch().status()
    }

    #[test]
    fn unknown_user_is_404() {
        let _g = lock();
        let client = setup();
        seed("s7-root", true, None);
        let status = put_sharing(
            &client,
            Some(cookie("s7-root", true)),
            &json!({ "userId": "s7-ghost", "enabled": false }),
        );
        assert_eq!(status, Status::NotFound);
    }

    #[test]
    fn bad_id_is_400() {
        let _g = lock();
        let client = setup();
        seed("s7-root", true, None);
        let status = put_sharing(
            &client,
            Some(cookie("s7-root", true)),
            &json!({ "userId": "   ", "enabled": false }),
        );
        assert_eq!(status, Status::BadRequest);
    }

    #[test]
    fn non_admin_is_401() {
        let _g = lock();
        let client = setup();
        seed("s7-root", true, None);
        seed("s7-bob", false, None);
        let status = put_sharing(
            &client,
            Some(cookie("s7-bob", false)),
            &json!({ "userId": "s7-root", "enabled": false }),
        );
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn disabling_sharing_of_sole_admin_is_allowed() {
        let _g = lock();
        let client = setup();
        seed("s7-sole", true, None);
        let status = put_sharing(
            &client,
            Some(cookie("s7-sole", true)),
            &json!({ "userId": "s7-sole", "enabled": false }),
        );
        assert_eq!(status, Status::Ok);
        let rec = crate::auth::users::user_record("s7-sole")
            .expect("get user")
            .expect("exists");
        assert!(!rec.shares_enabled);
    }
}
