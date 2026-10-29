use rocket::http::Status;
use rocket::put;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::auth::users::{self, validate_user_id};
use crate::openapi_components::Unauthorized;
use crate::router::auth::{GuardAdmin, GuardMutate, Role};
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// Body for `PUT /put/users/admin`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetUserAdminRequest {
    pub user_id: String,
    pub admin: bool,
}

/// Grant or revoke the admin role for `target`.
///
/// The zero-admin check and the write happen in one redb write transaction
/// (see [`users::set_admin_role`]), so a change that would leave zero admins
/// is refused. A same-flag write is a no-op reporting success without an
/// audit line.
///
/// # Errors
/// Returns 400 when the id is invalid, 404 when the target user is unknown,
/// 409 when the demotion would leave zero admins, and 500 when the store
/// fails.
fn set_admin_sync(actor_id: &str, target_user_id: &str, admin: bool) -> AppResult<bool> {
    let target = validate_user_id(target_user_id)?;
    let changed = users::set_admin_role(&target, admin)?;
    if changed {
        log::info!("admin role change: actor={actor_id} target={target} admin={admin}");
    }
    Ok(changed)
}

/// Grant or revoke a user's admin role.
///
/// Admin-only: a non-admin, share, or missing credential is denied at
/// dispatch with 401. A change that would leave zero admins is refused,
/// whether the target is the caller or the sole other admin; self-demotion
/// with another admin present succeeds (transfer). No-op same-flag writes
/// succeed without an audit line.
///
/// Corner cases: Unknown `userId` names no user. The caller id is read from
/// the dispatch-validated claims, never from the body.
///
/// Errors: 400 invalid `userId` — 401 missing or invalid credentials,
/// non-admin caller, or share credentials — 404 unknown `userId` — 405
/// read-only mode — 409 demotion would leave zero admins — 500 storage
/// failure.
#[utoipa::path(
        tag = "auth",
        request_body = SetUserAdminRequest,
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
            (status = 200, description = "Admin role updated"),
            (status = 400, description = "Invalid input"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 404, description = "User not found"),
            (status = 405, description = "Read-only mode"),
            (status = 409, description = "Demotion would leave zero admins"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/users/admin", data = "<body>")]
pub fn set_user_admin_handler(
    auth: GuardResult<GuardAdmin>,
    read_only: GuardResult<GuardMutate>,
    body: Json<SetUserAdminRequest>,
) -> AppResult<Status> {
    let _ = read_only?;
    let auth = auth?;
    let GuardAdmin { claims } = auth;
    let Role::User { id: actor_id, .. } = claims.role else {
        return Err(AppError::new(ErrorKind::Auth, "Share token not accepted"));
    };
    let req = body.into_inner();
    set_admin_sync(&actor_id, &req.user_id, req.admin)?;
    Ok(Status::Ok)
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Cookie, Header, Status};
    use serde_json::json;

    use crate::tests::bootstrap::{cookie, lock, seed, setup};

    fn put_admin(
        client: &rocket::local::blocking::Client,
        cookie: Option<Cookie<'static>>,
        body: &serde_json::Value,
    ) -> Status {
        let mut req = client
            .put("/put/users/admin")
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
        seed("s5-root", true, None);
        let status = put_admin(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "s5-ghost", "admin": true }),
        );
        assert_eq!(status, Status::NotFound);
    }

    /// Unknown fields on the admin-role body are a 422: a typo'd key
    /// must not be silently dropped.
    #[test]
    fn unknown_field_on_admin_body_is_422() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-bob", false, None);
        let status = put_admin(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "s5-bob", "admin": true, "password": "x" }),
        );
        assert_eq!(
            status,
            Status::UnprocessableEntity,
            "unknown fields must be rejected, not ignored"
        );
    }

    #[test]
    fn sole_admin_demoting_self_is_409() {
        let _g = lock();
        let client = setup();
        seed("s5-sole", true, None);
        let status = put_admin(
            &client,
            Some(cookie("s5-sole", true)),
            &json!({ "userId": "s5-sole", "admin": false }),
        );
        assert_eq!(status, Status::Conflict);
    }

    #[test]
    fn demoting_sole_other_admin_is_409() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-victim", true, None);
        // Demote the other admin first so only the victim remains, then
        // demoting the victim must be refused.
        let status = put_admin(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "s5-root", "admin": false }),
        );
        assert_eq!(status, Status::Ok);
        let status = put_admin(
            &client,
            Some(cookie("s5-victim", true)),
            &json!({ "userId": "s5-victim", "admin": false }),
        );
        assert_eq!(status, Status::Conflict);
    }

    #[test]
    fn self_demotion_with_other_admin_present_is_200() {
        let _g = lock();
        let client = setup();
        seed("s5-a", true, None);
        seed("s5-b", true, None);
        let status = put_admin(
            &client,
            Some(cookie("s5-a", true)),
            &json!({ "userId": "s5-a", "admin": false }),
        );
        assert_eq!(status, Status::Ok);
        let admin = crate::auth::users::get_admin("s5-a")
            .expect("get admin")
            .unwrap_or(false);
        assert!(!admin, "admin flag must be cleared");
        // The demotion bumps the user's token version: the role travels in
        // the token, so the outstanding tokens die and the new role appears
        // at the next sign-in.
        let record = crate::auth::users::user_record("s5-a")
            .expect("get user")
            .expect("record");
        assert_eq!(record.ver, 1, "the demotion must invalidate the tokens");
    }

    #[test]
    fn grant_and_noop_write_are_200() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-bob", false, None);
        let status = put_admin(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "s5-bob", "admin": true }),
        );
        assert_eq!(status, Status::Ok);
        let status = put_admin(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "s5-bob", "admin": true }),
        );
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn non_admin_caller_is_401() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-bob", false, None);
        let status = put_admin(
            &client,
            Some(cookie("s5-bob", false)),
            &json!({ "userId": "s5-bob", "admin": true }),
        );
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn share_headers_without_cookie_is_401() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-bob", false, None);
        let status = client
            .put("/put/users/admin")
            .header(ContentType::JSON)
            .header(Header::new("x-album-id", "s5-album"))
            .header(Header::new("x-share-id", "s5-share"))
            .body(json!({ "userId": "s5-bob", "admin": true }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn unauthenticated_is_401() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        let status = put_admin(
            &client,
            None,
            &json!({ "userId": "s5-root", "admin": false }),
        );
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn bad_id_is_400() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        let status = put_admin(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "   ", "admin": true }),
        );
        assert_eq!(status, Status::BadRequest);
        let long = "a".repeat(65);
        let status = put_admin(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": long, "admin": true }),
        );
        assert_eq!(status, Status::BadRequest);
    }

    #[test]
    fn read_only_mode_is_405() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-bob", false, None);
        crate::tests::bootstrap::write_config(&json!({ "read_only_mode": true }));
        let status = put_admin(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "s5-bob", "admin": true }),
        );
        assert_eq!(status, Status::MethodNotAllowed);
    }
}
