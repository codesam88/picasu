use rocket::http::Status;
use rocket::put;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::auth::users::{self, validate_user_id};
use crate::openapi_components::Unauthorized;
use crate::router::auth::{GuardAdmin, GuardMutate, Role};
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// Body for `PUT /put/users/disabled`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetUserDisabledRequest {
    pub user_id: String,
    pub disabled: bool,
}

/// Lock or unlock a user account.
///
/// The last-enabled-admin check and the write happen in one redb write
/// transaction (see [`users::set_disabled_role`]), so disabling the final
/// enabled admin is refused. A same-flag write is a no-op reporting success
/// without an audit line. Disabling bumps the user's token version so
/// outstanding sessions die; re-enabling leaves it untouched.
///
/// # Errors
/// Returns 400 when the id is invalid, 404 when the target user is unknown,
/// 409 when disabling would leave zero enabled admins, and 500 when the
/// store fails.
fn set_disabled_sync(actor_id: &str, target_user_id: &str, disabled: bool) -> AppResult<bool> {
    let target = validate_user_id(target_user_id)?;
    let changed = users::set_disabled_role(&target, disabled)?;
    if changed {
        log::info!("account disabled change: actor={actor_id} target={target} disabled={disabled}");
    }
    Ok(changed)
}

/// Lock or unlock a user account (admin-only).
///
/// Locked accounts cannot log in and their outstanding sessions die;
/// personal data and shares are untouched, and re-enabling restores
/// access with a fresh login.
#[utoipa::path(
        tag = "auth",
        request_body = SetUserDisabledRequest,
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
            (status = 200, description = "Account disabled state updated"),
            (status = 400, description = "Invalid input"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 404, description = "User not found"),
            (status = 405, description = "Read-only mode"),
            (status = 409, description = "Disabling would leave zero enabled admins"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/users/disabled", data = "<body>")]
pub fn set_user_disabled_handler(
    auth: GuardResult<GuardAdmin>,
    read_only: GuardResult<GuardMutate>,
    body: Json<SetUserDisabledRequest>,
) -> AppResult<Status> {
    let _ = read_only?;
    let auth = auth?;
    let GuardAdmin { claims } = auth;
    let Role::User { id: actor_id, .. } = claims.role else {
        return Err(AppError::new(ErrorKind::Auth, "Share token not accepted"));
    };
    let req = body.into_inner();
    set_disabled_sync(&actor_id, &req.user_id, req.disabled)?;
    Ok(Status::Ok)
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Cookie, Status};
    use serde_json::json;

    use crate::tests::bootstrap::{cookie, lock, login, seed, setup};
    use crate::tests::fixtures::test_password;

    fn put_disabled(
        client: &rocket::local::blocking::Client,
        cookie: Option<Cookie<'static>>,
        body: &serde_json::Value,
    ) -> Status {
        let mut req = client
            .put("/put/users/disabled")
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
        let status = put_disabled(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "s5-ghost", "disabled": true }),
        );
        assert_eq!(status, Status::NotFound);
    }

    #[test]
    fn sole_enabled_admin_disable_is_409() {
        let _g = lock();
        let client = setup();
        seed("s5-sole", true, None);
        let status = put_disabled(
            &client,
            Some(cookie("s5-sole", true)),
            &json!({ "userId": "s5-sole", "disabled": true }),
        );
        assert_eq!(status, Status::Conflict);
    }

    #[test]
    fn non_admin_caller_is_401() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-bob", false, None);
        let status = put_disabled(
            &client,
            Some(cookie("s5-bob", false)),
            &json!({ "userId": "s5-bob", "disabled": true }),
        );
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn unauthenticated_is_401() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        let status = put_disabled(
            &client,
            None,
            &json!({ "userId": "s5-root", "disabled": true }),
        );
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn bad_id_is_400() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        let status = put_disabled(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "   ", "disabled": true }),
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
        let status = put_disabled(
            &client,
            Some(cookie("s5-root", true)),
            &json!({ "userId": "s5-bob", "disabled": true }),
        );
        assert_eq!(status, Status::MethodNotAllowed);
    }

    #[test]
    fn disabled_login_rejected_like_wrong_password() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-u1", false, Some(test_password("t-s5-u1").as_str()));
        let pw = test_password("t-s5-u1");
        assert_eq!(
            crate::tests::bootstrap::login_status(&client, "s5-u1", &pw),
            Status::Ok,
            "login works before disable"
        );

        assert_eq!(
            put_disabled(
                &client,
                Some(cookie("s5-root", true)),
                &json!({ "userId": "s5-u1", "disabled": true }),
            ),
            Status::Ok,
            "disable"
        );
        assert_eq!(
            crate::tests::bootstrap::login_status(&client, "s5-u1", &pw),
            Status::Unauthorized,
            "disabled login rejected"
        );
        assert_eq!(
            crate::tests::bootstrap::login_status(&client, "s5-u1", "definitely wrong"),
            Status::Unauthorized,
            "wrong password rejected the same way"
        );
    }

    #[test]
    fn live_session_dies_on_disable_and_stays_dead() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-u2", false, Some(test_password("t-s5-u2").as_str()));
        let pw = test_password("t-s5-u2");
        let (status, _, token) = login(&client, "s5-u2", &pw);
        assert_eq!(status, Status::Ok, "login before disable");
        let jar = Cookie::new("jwt", token.expect("session token"));
        let profile = |jar: Cookie<'static>| {
            client
                .get("/get/self/profile")
                .cookie(jar)
                .dispatch()
                .status()
        };
        assert_eq!(profile(jar.clone()), Status::Ok);

        assert_eq!(
            put_disabled(
                &client,
                Some(cookie("s5-root", true)),
                &json!({ "userId": "s5-u2", "disabled": true }),
            ),
            Status::Ok,
            "disable"
        );
        assert_eq!(
            profile(jar.clone()),
            Status::Unauthorized,
            "live session dies on disable"
        );

        assert_eq!(
            put_disabled(
                &client,
                Some(cookie("s5-root", true)),
                &json!({ "userId": "s5-u2", "disabled": false }),
            ),
            Status::Ok,
            "re-enable"
        );
        assert_eq!(
            profile(jar),
            Status::Unauthorized,
            "old session stays dead after re-enable (ver was bumped)"
        );
        assert_eq!(
            crate::tests::bootstrap::login_status(&client, "s5-u2", &pw),
            Status::Ok,
            "fresh login works after re-enable"
        );
    }

    #[test]
    fn listing_shows_disabled_flag() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-u3", false, None);
        let listed: Vec<serde_json::Value> = client
            .get("/get/users")
            .cookie(cookie("s5-root", true))
            .dispatch()
            .into_json()
            .expect("users json");
        let entry = listed
            .iter()
            .find(|u| u["userId"].as_str() == Some("s5-u3"))
            .expect("user listed");
        assert_eq!(entry["disabled"], false);

        assert_eq!(
            put_disabled(
                &client,
                Some(cookie("s5-root", true)),
                &json!({ "userId": "s5-u3", "disabled": true }),
            ),
            Status::Ok
        );
        let listed: Vec<serde_json::Value> = client
            .get("/get/users")
            .cookie(cookie("s5-root", true))
            .dispatch()
            .into_json()
            .expect("users json");
        let entry = listed
            .iter()
            .find(|u| u["userId"].as_str() == Some("s5-u3"))
            .expect("user listed");
        assert_eq!(entry["disabled"], true);
    }

    #[test]
    fn freshly_minted_session_rejected_while_disabled() {
        let _g = lock();
        let client = setup();
        seed("s5-root", true, None);
        seed("s5-u4", false, None);
        assert_eq!(
            put_disabled(
                &client,
                Some(cookie("s5-root", true)),
                &json!({ "userId": "s5-u4", "disabled": true }),
            ),
            Status::Ok
        );
        // `cookie()` mints against the current record version, so the
        // version check passes by construction — only the disabled check
        // can refuse this request.
        let status = client
            .get("/get/self/profile")
            .cookie(cookie("s5-u4", false))
            .dispatch()
            .status();
        assert_eq!(status, Status::Unauthorized);
    }
}
