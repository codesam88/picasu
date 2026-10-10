use rocket::http::Status;
use rocket::put;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::auth::users::{self, validate_user_id};
use crate::openapi_components::Unauthorized;
use crate::router::auth::{GuardAuth, GuardReadOnlyMode, Role};
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// Body for `PUT /put/users/admin`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
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
        responses(
            (status = 200, description = "Admin role updated"),
            (status = 400, description = "Invalid input"),
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
    auth: GuardAuth,
    read_only: GuardResult<GuardReadOnlyMode>,
    body: Json<SetUserAdminRequest>,
) -> AppResult<Status> {
    let _ = read_only?;
    let GuardAuth { claims } = auth;
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

    use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD, make_client, reset_backend_state};

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn setup() -> rocket::local::blocking::Client {
        let _ = &*TEST_ENV;
        reset_backend_state();
        make_client()
    }

    fn cookie(id: &str, admin: bool) -> Cookie<'static> {
        let token = crate::router::auth::Claims::new_user(id.to_string(), admin).encode();
        Cookie::new("jwt", token)
    }

    fn seed(id: &str, admin: bool) {
        crate::auth::users::create_user(id, admin).expect("seed user");
    }

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
        seed("s5-root", true);
        let status = put_admin(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "s5-ghost", "admin": true }),
        );
        assert_eq!(status, Status::NotFound);
    }

    #[test]
    fn sole_admin_demoting_self_is_409() {
        let _g = lock();
        let client = setup();
        seed("s5-sole", true);
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
        seed("s5-root", true);
        seed("s5-victim", true);
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
        seed("s5-a", true);
        seed("s5-b", true);
        let status = put_admin(
            &client,
            Some(cookie("s5-a", true)),
            &json!({ "userId": "s5-a", "admin": false }),
        );
        assert_eq!(status, Status::Ok);
        let record = crate::auth::users::get_user("s5-a").expect("get user");
        assert_eq!(
            record,
            Some(crate::auth::users::UserRecord { admin: false })
        );
    }

    #[test]
    fn grant_and_noop_write_are_200() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true);
        seed("s5-bob", false);
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
        seed("s5-root", true);
        seed("s5-bob", false);
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
        seed("s5-root", true);
        seed("s5-bob", false);
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
        seed("s5-root", true);
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
        seed("s5-root", true);
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
        seed("s5-root", true);
        seed("s5-bob", false);
        crate::tests::bootstrap::write_config(&json!({ "read_only_mode": true }));
        let status = put_admin(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "s5-bob", "admin": true }),
        );
        assert_eq!(status, Status::MethodNotAllowed);
    }
}
