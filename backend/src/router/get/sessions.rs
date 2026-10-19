use rocket::get;
use rocket::serde::json::Json;

use crate::auth::users::{self, SessionRecord};
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardUser;
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// List the caller's live sessions.
///
/// Returns the live `SessionRecord` array (`jti` + `issuedAt`) for the
/// authenticated user only. Any authenticated user may call it; it never
/// returns another user's sessions.
///
/// Errors: 401 missing or invalid credentials — 403 closed install (no
/// administrator enrolled) — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        responses(
            (status = 200, description = "Live sessions", body = Vec<SessionRecord>),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/get/sessions")]
pub fn list_sessions_handler(user: GuardResult<GuardUser>) -> AppResult<Json<Vec<SessionRecord>>> {
    let user = user?;
    let user_id = match &user.claims.role {
        crate::router::auth::Role::User { id, .. } => id.clone(),
        crate::router::auth::Role::Share(_) => {
            return Err(AppError::new(ErrorKind::Auth, "Share token not accepted")
                .context("Authentication error"));
        }
    };
    let record =
        users::get_user(&user_id).map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
    match record {
        Some(r) => Ok(Json(r.sessions)),
        None => Err(AppError::new(ErrorKind::Auth, "Unknown user").context("Authentication error")),
    }
}

#[cfg(test)]
mod tests {
    use rocket::http::{Cookie, Status};

    use crate::auth::users::SessionRecord;
    use crate::tests::bootstrap::{cookie, lock, setup};

    #[test]
    fn unauthenticated_inventory_is_401() {
        let _g = lock();
        let client = setup();
        crate::auth::users::create_user("s6-sess-unauth", true).expect("seed");
        let resp = client.get("/get/sessions").dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
    }

    #[test]
    fn inventory_lists_live_sessions() {
        let _g = lock();
        let client = setup();
        crate::auth::users::create_user("s6-sess-list", true).expect("seed");
        let c1 = cookie("s6-sess-list", true);
        let t1 = c1.value().to_string();
        // Decode jti for later assertion
        let jti1 = crate::router::auth::decode_typed::<crate::router::auth::Claims>(
            &t1,
            &crate::router::auth::VALIDATION,
        )
        .expect("decode")
        .jti;
        let c2 = cookie("s6-sess-list", true);
        let t2 = c2.value().to_string();
        let jti2 = crate::router::auth::decode_typed::<crate::router::auth::Claims>(
            &t2,
            &crate::router::auth::VALIDATION,
        )
        .expect("decode")
        .jti;

        // Both sessions visible
        let resp = client
            .get("/get/sessions")
            .cookie(Cookie::new("jwt", t1.clone()))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("body");
        let listed: Vec<SessionRecord> = serde_json::from_str(&body).expect("json");
        assert_eq!(listed.len(), 2);
        let jtis: Vec<_> = listed.iter().map(|s| s.jti.as_str()).collect();
        assert!(jtis.contains(&jti1.as_str()));
        assert!(jtis.contains(&jti2.as_str()));

        // Logout one session, inventory shrinks to one with matching jti
        let resp = client
            .post("/post/logout")
            .cookie(Cookie::new("jwt", t1.clone()))
            .dispatch();
        assert_eq!(resp.status(), Status::NoContent);
        let resp = client
            .get("/get/sessions")
            .cookie(Cookie::new("jwt", t2.clone()))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("body");
        let listed: Vec<SessionRecord> = serde_json::from_str(&body).expect("json");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].jti, jti2);
    }
}
