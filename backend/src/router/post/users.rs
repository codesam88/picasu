use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::auth::users;
use crate::error::ResultExt;
use crate::model::config::APP_CONFIG;
use crate::openapi_components::Unauthorized;
use crate::router::auth::{GuardAuth, GuardReadOnlyMode};
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// Summary of a user record. Password hashes are never exported.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
    pub user_id: String,
    pub admin: bool,
}

/// Body for `POST /post/users/create`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub user_id: String,
    pub password: String,
    pub admin: bool,
}

/// Create a user record plus its password hash and persist both.
///
/// Thin delegate over [`users::provision_user`]: id/password validation
/// (including the default password policy for new passwords), duplicate rejection,
/// redb record insert, and password-store write all live there. The
/// trimmed (canonical) password is stored, matching the legacy semantics
/// that trimmed everywhere; login verifies the trimmed input, so
/// write-then-login stays consistent.
///
/// # Errors
/// Returns 400 when the id is invalid or the password violates the
/// policy, 409 when the id already exists, and 500 when either store
/// fails.
fn create_user_sync(user_id: &str, password: &str, admin: bool) -> AppResult<UserSummary> {
    let id = users::provision_user(user_id, password, admin)?;
    Ok(UserSummary { user_id: id, admin })
}

/// Create a user.
///
/// Admin-only, except for the bootstrap exception: while the user store is
/// empty an unauthenticated call succeeds and forces `admin: true`
/// regardless of the body's flag — but only in open mode. With a legacy
/// password configured the store stays gated by that host secret (review
/// A1): unauthenticated create is denied and enrollment happens through
/// `POST /post/authenticate` migration. A non-admin caller — or no caller
/// once any user exists — is rejected.
///
/// Corner cases: The `admin` flag is honored for admin callers and forced
/// on for bootstrap. The password is trimmed before hashing.
///
/// Errors: 400 invalid `userId` or password violating the policy — 401
/// missing or invalid credentials, non-admin caller, legacy-mode
/// unauthenticated call, or unauthenticated call once any user exists —
/// 405 read-only mode — 409 `userId` already exists — 500 storage
/// failure.
#[utoipa::path(
        tag = "auth",
        request_body = CreateUserRequest,
        responses(
            (status = 200, description = "User created", body = UserSummary),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 409, description = "User already exists"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/users/create", data = "<body>")]
pub async fn create_user_handler(
    auth: GuardResult<GuardAuth>,
    read_only: GuardResult<GuardReadOnlyMode>,
    body: Json<CreateUserRequest>,
) -> AppResult<Json<UserSummary>> {
    let _ = read_only?;
    let non_empty =
        users::user_count().map_err(|e| AppError::from_err(ErrorKind::Database, e))? > 0;
    // Bootstrap exception: while the store is empty every creation is the
    // first user, forced to admin regardless of the body's flag — whether
    // the caller is unauthenticated or carries the open-mode ephemeral
    // identity (which `GuardAuth` accepts as `Ok`).
    let admin = match auth {
        Ok(guard) if non_empty => {
            if !guard.claims.is_admin() {
                return Err(AppError::new(ErrorKind::Auth, "Admin required")
                    .context("User creation failed"));
            }
            body.admin
        }
        Ok(_) => true,
        Err(err) => {
            // Empty-store-only, and only while open: with a legacy password
            // configured the host secret gates enrollment too (review A1) —
            // an unauthenticated create is denied and migration happens
            // through `POST /post/authenticate` instead.
            let legacy_password = APP_CONFIG
                .get()
                .expect("APP_CONFIG not initialized")
                .read()
                .expect("lock poisoned")
                .password
                .is_some();
            if non_empty || legacy_password {
                return Err(err);
            }
            true
        }
    };
    let req = body.into_inner();
    let created = spawn_blocking(move || create_user_sync(&req.user_id, &req.password, admin))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
    Ok(Json(created))
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Cookie, Status};
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

    fn admin_cookie(id: &str, admin: bool) -> Cookie<'static> {
        let token = crate::router::auth::Claims::new_user(id.to_string(), admin).encode();
        Cookie::new("jwt", token)
    }

    fn seed_admin_user() {
        crate::auth::users::create_user("s4-root", true).expect("seed admin");
    }

    #[test]
    fn admin_creates_user_and_non_admin() {
        let _g = lock();
        let client = setup();
        seed_admin_user();
        let cookie = admin_cookie("s4-root", true);
        let resp = client
            .post("/post/users/create")
            .cookie(cookie.clone())
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "s4-alice", "password": "s4-alice-pw12", "admin": false })
                    .to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let created: super::UserSummary =
            serde_json::from_str(&resp.into_string().expect("body")).expect("created user body");
        assert_eq!(created.user_id, "s4-alice");
        assert!(!created.admin);
        let record = crate::auth::users::get_user("s4-alice").expect("get user");
        assert_eq!(
            record,
            Some(crate::auth::users::UserRecord { admin: false })
        );
        // Admin flag honored for a second, admin creation.
        let resp = client
            .post("/post/users/create")
            .cookie(cookie)
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "s4-other-admin", "password": "s4-other-pw12", "admin": true })
                    .to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
    }

    #[test]
    fn duplicate_user_is_409() {
        let _g = lock();
        let client = setup();
        seed_admin_user();
        let cookie = admin_cookie("s4-root", true);
        let body =
            json!({ "userId": "s4-dupe", "password": "s4-dupe-pw12", "admin": false }).to_string();
        let resp = client
            .post("/post/users/create")
            .cookie(cookie.clone())
            .header(ContentType::JSON)
            .body(body.clone())
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let resp = client
            .post("/post/users/create")
            .cookie(cookie)
            .header(ContentType::JSON)
            .body(body)
            .dispatch();
        assert_eq!(resp.status(), Status::Conflict);
    }

    #[test]
    fn non_admin_create_is_401() {
        let _g = lock();
        let client = setup();
        seed_admin_user();
        crate::auth::users::create_user("s4-bob", false).expect("seed bob");
        let cookie = admin_cookie("s4-bob", false);
        let resp = client
            .post("/post/users/create")
            .cookie(cookie)
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "s4-eve", "password": "s4-eve-pw123", "admin": false })
                    .to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
    }

    #[test]
    fn unauthenticated_create_with_non_empty_store_is_401() {
        let _g = lock();
        let client = setup();
        seed_admin_user();
        let resp = client
            .post("/post/users/create")
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "s4-eve", "password": "s4-eve-pw123", "admin": false })
                    .to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
    }

    #[test]
    fn bootstrap_create_forces_admin() {
        let _g = lock();
        let client = setup();
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        let resp = client
            .post("/post/users/create")
            .header(ContentType::JSON)
            .body(
                json!({ "userId": "root", "password": "s4-root-pw12", "admin": false }).to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let created: super::UserSummary =
            serde_json::from_str(&resp.into_string().expect("body")).expect("created user body");
        assert!(created.admin, "bootstrap must force admin:true");
        let record = crate::auth::users::get_user("root").expect("get user");
        assert_eq!(record, Some(crate::auth::users::UserRecord { admin: true }));
    }
}
