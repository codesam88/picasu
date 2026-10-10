use rocket::http::Status;
use rocket::put;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::auth::users::{self, validate_user_id};
use crate::error::ResultExt;
use crate::openapi_components::Unauthorized;
use crate::router::auth::{GuardReadOnlyMode, GuardUser};
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// Body for `PUT /put/users/password`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetUserPasswordRequest {
    pub user_id: String,
    pub old_password: Option<String>,
    pub new_password: String,
}

/// Set the password for `target_user_id` on behalf of `caller_id`.
///
/// The new password is trimmed once and the trimmed (canonical) form is
/// stored, matching the legacy semantics that trimmed everywhere; login
/// verifies the trimmed input, so write-then-login stays consistent. The
/// old password, when required, is trimmed the same way before comparison.
///
/// Rules: an admin may set any user's password without the old one; a
/// non-admin may only set their own, presenting the correct old password.
/// With an empty store the target cannot exist yet, so the first user is
/// bootstrapped as admin (forced), mirroring user creation — this keeps
/// the legacy `PUT /put/config/password` first-run flow working.
///
/// # Errors
/// Returns 400 when the id is invalid or the trimmed new password is
/// empty, 401 when the old password is wrong, 403 when a non-admin targets
/// another user, 404 when the target user is unknown and the store is
/// non-empty, and 500 when either store fails.
pub(crate) fn set_password_sync(
    caller_id: &str,
    caller_admin: bool,
    target_user_id: &str,
    old_password: Option<&str>,
    new_password: &str,
) -> AppResult<()> {
    let target = validate_user_id(target_user_id)?;
    let canonical_new = new_password.trim().to_string();
    if canonical_new.is_empty() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "new password must not be empty",
        ));
    }
    let target_exists = users::get_user(&target)
        .map_err(|e| AppError::from_err(ErrorKind::Database, e))?
        .is_some();
    if !target_exists {
        let empty =
            users::user_count().map_err(|e| AppError::from_err(ErrorKind::Database, e))? == 0;
        if empty {
            users::create_user(&target, true)
                .map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
        } else {
            return Err(AppError::new(
                ErrorKind::NotFound,
                format!("unknown user: {target}"),
            ));
        }
    }
    if !caller_admin {
        if target != caller_id {
            return Err(AppError::new(
                ErrorKind::PermissionDenied,
                "cannot change another user's password",
            ));
        }
        let presented = old_password.unwrap_or("").trim();
        let path = users::passwd_file_path();
        let store = crate::auth::password::PasswdFile::load(&path)
            .map_err(|e| AppError::from_err(ErrorKind::IO, e))?;
        if !store.verify(&target, presented) {
            return Err(AppError::new(ErrorKind::Auth, "Incorrect current password")
                .context("Password change failed"));
        }
    }
    let path = users::passwd_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::from_err(ErrorKind::IO, e.into()))?;
    }
    let mut store = crate::auth::password::PasswdFile::load(&path)
        .map_err(|e| AppError::from_err(ErrorKind::IO, e))?;
    store
        .set_password(&target, &canonical_new)
        .map_err(|e| AppError::from_err(ErrorKind::IO, e))?;
    Ok(())
}

pub(crate) fn caller_identity(claims: &crate::router::auth::Claims) -> (String, bool) {
    match &claims.role {
        crate::router::auth::Role::User { id, admin } => (id.clone(), *admin),
        crate::router::auth::Role::Share(_) => (String::new(), false),
    }
}

/// Change a user's password.
///
/// An admin may set any user's password without the old one; a non-admin
/// may only change their own, presenting the correct `oldPassword`. The new
/// password is trimmed and must be non-empty after trimming (there is no
/// "clear to open": open mode is store-empty only).
///
/// Corner cases: Any authenticated user reaches this route (share tokens
/// are rejected); unauthenticated callers are denied at dispatch.
///
/// Errors: 400 invalid `userId` or empty `newPassword` — 401 missing or
/// invalid credentials, or wrong `oldPassword` — 403 non-admin targeting
/// another user — 404 unknown `userId` — 405 read-only mode — 500 storage
/// failure.
#[utoipa::path(
        tag = "auth",
        request_body = SetUserPasswordRequest,
        responses(
            (status = 200, description = "Password updated"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 404, description = "User not found"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/users/password", data = "<body>")]
pub async fn set_user_password_handler(
    user: GuardUser,
    read_only: GuardResult<GuardReadOnlyMode>,
    body: Json<SetUserPasswordRequest>,
) -> AppResult<Status> {
    let _ = read_only?;
    let (caller_id, caller_admin) = caller_identity(&user.claims);
    let req = body.into_inner();
    spawn_blocking(move || {
        set_password_sync(
            &caller_id,
            caller_admin,
            &req.user_id,
            req.old_password.as_deref(),
            &req.new_password,
        )
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
    Ok(Status::Ok)
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

    fn cookie(id: &str, admin: bool) -> Cookie<'static> {
        let token = crate::router::auth::Claims::new_user(id.to_string(), admin).encode();
        Cookie::new("jwt", token)
    }

    fn seed(id: &str, admin: bool, password: Option<&str>) {
        crate::auth::users::create_user(id, admin).expect("seed user");
        if let Some(pw) = password {
            let path = crate::auth::users::passwd_file_path();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("auth dir");
            }
            let mut store = crate::auth::password::PasswdFile::load(&path).expect("load passwd");
            store.set_password(id, pw).expect("set password");
        }
    }

    fn put_password(
        client: &rocket::local::blocking::Client,
        cookie: Cookie<'static>,
        body: &serde_json::Value,
    ) -> Status {
        client
            .put("/put/users/password")
            .cookie(cookie)
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch()
            .status()
    }

    #[test]
    fn admin_sets_any_password_without_old() {
        let _g = lock();
        let client = setup();
        seed("s4d-root", true, None);
        seed("s4d-bob", false, Some("s4d-bob-old"));
        let status = put_password(
            &client,
            cookie("s4d-root", true),
            &json!({ "userId": "s4d-bob", "newPassword": "s4d-bob-new" }),
        );
        assert_eq!(status, Status::Ok);
        let store =
            crate::auth::password::PasswdFile::load(&crate::auth::users::passwd_file_path())
                .expect("load passwd");
        assert!(store.verify("s4d-bob", "s4d-bob-new"));
    }

    #[test]
    fn self_service_with_correct_old_password() {
        let _g = lock();
        let client = setup();
        seed("s4d-carol", false, Some("s4d-carol-old"));
        let status = put_password(
            &client,
            cookie("s4d-carol", false),
            &json!({ "userId": "s4d-carol", "oldPassword": "s4d-carol-old", "newPassword": "s4d-carol-new" }),
        );
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn self_service_with_wrong_old_password_is_401() {
        let _g = lock();
        let client = setup();
        seed("s4d-dave", false, Some("s4d-dave-old"));
        let status = put_password(
            &client,
            cookie("s4d-dave", false),
            &json!({ "userId": "s4d-dave", "oldPassword": "nope", "newPassword": "s4d-dave-new" }),
        );
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn cross_user_by_non_admin_is_403() {
        let _g = lock();
        let client = setup();
        seed("s4d-erin", false, Some("s4d-erin-old"));
        seed("s4d-frank", false, Some("s4d-frank-old"));
        let status = put_password(
            &client,
            cookie("s4d-erin", false),
            &json!({ "userId": "s4d-frank", "oldPassword": "s4d-frank-old", "newPassword": "s4d-frank-new" }),
        );
        assert_eq!(status, Status::Forbidden);
    }

    #[test]
    fn empty_new_password_is_400() {
        let _g = lock();
        let client = setup();
        seed("s4d-root", true, None);
        let status = put_password(
            &client,
            cookie("s4d-root", true),
            &json!({ "userId": "s4d-root", "newPassword": "   " }),
        );
        assert_eq!(status, Status::BadRequest);
    }

    #[test]
    fn empty_store_bootstraps_first_user_as_admin() {
        let _g = lock();
        let client = setup();
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        // No cookie: open first-run mode admits the GuardUser ephemeral id.
        let status = client
            .put("/put/users/password")
            .header(ContentType::JSON)
            .body(json!({ "userId": "first", "newPassword": "s4d-first-pw" }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
        let record = crate::auth::users::get_user("first").expect("get user");
        assert_eq!(record, Some(crate::auth::users::UserRecord { admin: true }));
    }
}
