use rocket::get;
use rocket::serde::json::Json;

use crate::auth::users;
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::router::post::users_register::UserSummary;
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// List all users as `{ userId, admin }` pairs ordered by id.
///
/// Password hashes are never exported. Admin-only: a non-admin or missing
/// credential is denied at dispatch with 401.
///
/// Errors: 401 missing or invalid admin credentials; share tokens are not
/// accepted — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        responses(
            (status = 200, description = "User list", body = Vec<UserSummary>),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/get/users")]
pub fn list_users_handler(auth: GuardResult<GuardAuth>) -> AppResult<Json<Vec<UserSummary>>> {
    let _ = auth?;
    // An empty store has no admins, so an admin-only route refuses in all
    // states; enrollment state is served by GET /get/server-info.
    if crate::auth::user_store::count(&users::user_store_path()) == 0 {
        return Err(AppError::new(
            ErrorKind::Auth,
            crate::router::auth::not_enrolled_message(),
        ));
    }
    let rows = users::list_users().map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
    Ok(Json(
        rows.into_iter()
            .map(|(user_id, record)| UserSummary {
                user_id,
                admin: record.admin,
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use rocket::http::Status;

    use crate::router::post::users_register::UserSummary;
    use crate::tests::bootstrap::{cookie, lock, setup};

    #[test]
    fn admin_lists_users_without_hashes() {
        let _g = lock();
        let client = setup();
        crate::auth::users::create_user("s4c-bob", false).expect("seed bob");
        crate::auth::users::create_user("s4c-root", true).expect("seed root");
        let resp = client
            .get("/get/users")
            .cookie(cookie("s4c-root", true))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("body");
        assert!(
            !body.contains("hash") && !body.contains("salt"),
            "hashes must never be exported: {body}"
        );
        let listed: Vec<UserSummary> = serde_json::from_str(&body).expect("user list body");
        assert_eq!(
            listed,
            vec![
                UserSummary {
                    user_id: "s4c-bob".to_string(),
                    admin: false,
                },
                UserSummary {
                    user_id: "s4c-root".to_string(),
                    admin: true,
                },
            ]
        );
    }

    #[test]
    fn non_admin_list_is_401() {
        let _g = lock();
        let client = setup();
        crate::auth::users::create_user("s4c-bob", false).expect("seed bob");
        let resp = client
            .get("/get/users")
            .cookie(cookie("s4c-bob", false))
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
    }

    #[test]
    fn unauthenticated_list_is_401() {
        let _g = lock();
        let client = setup();
        crate::auth::users::create_user("s4c-root", true).expect("seed root");
        let resp = client.get("/get/users").dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
    }

    #[test]
    fn empty_store_with_flag_on_anonymous_is_401() {
        let _g = lock();
        let client = setup();
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        // Enrollment state is served by GET /get/server-info; an empty
        // store has no admins, so this admin-only route refuses with the
        // window-aware enrollment hint in the body.
        let resp = client.get("/get/users").dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
    }
}
