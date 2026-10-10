use rocket::get;
use rocket::serde::json::Json;

use crate::auth::users;
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::router::post::users::UserSummary;
use crate::router::{AppError, AppResult, ErrorKind};

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
pub fn list_users_handler(_auth: GuardAuth) -> AppResult<Json<Vec<UserSummary>>> {
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
    use rocket::http::{Cookie, Status};

    use crate::router::post::users::UserSummary;
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
}
