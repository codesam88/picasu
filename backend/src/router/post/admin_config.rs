use log::error;
use rocket::http::Status;
use rocket::post;

use tokio::task::spawn_blocking;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::config::AppConfig;
use crate::openapi_components::Unauthorized;
use crate::router::auth::{GuardAuth, GuardReauth};
use crate::router::{AppResult, GuardResult};

/// Lift the read-only kill-switch.
///
/// Clears `readOnlyMode` and persists the change. Admin-only and deliberately
/// NOT gated by `GuardReadOnlyMode`: lifting the kill-switch must be callable
/// while it is on. Requires re-authentication via the `X-Picasu-Password`
/// header (403 when it is missing or wrong).
///
/// Errors: 401 missing or invalid admin credentials — 403 missing or wrong
/// re-authentication password — 500 config write failure.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    responses(
        (status = 200, description = "Read-only mode disabled"),
        (status = 401, response = Unauthorized),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Internal error"),
    )
)]
#[post("/post/admin/config/disable-read-only")]
pub async fn disable_read_only_handler(
    auth: GuardResult<GuardAuth>,
    reauth: GuardResult<GuardReauth>,
) -> AppResult<Status> {
    let _ = auth?;
    let _ = reauth?;
    spawn_blocking(move || -> Result<Status, AppError> {
        let mut current = crate::model::config::with_config(AppConfig::clone);
        current.read_only_mode = false;
        AppConfig::update(current).map_err(|e| {
            error!("Failed to update config: {e}");
            AppError::from_err(ErrorKind::Internal, e)
        })?;
        Ok(Status::Ok)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Task join error"))?
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Status};

    use crate::tests::bootstrap::{cookie, lock, seed, setup};

    #[test]
    fn disable_read_only_lifts_mode() {
        let _g = lock();
        let client = setup();
        seed("dar-root", true, None);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = true;
        let status = client
            .post("/post/admin/config/disable-read-only")
            .cookie(cookie("dar-root", true))
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "x-picasu-password",
                crate::tests::fixtures::test_password("dar-root"),
            ))
            .body("{}")
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
        let mode = crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .read()
            .expect("lock")
            .read_only_mode;
        assert!(!mode, "mode must be off");
    }

    #[test]
    fn disable_read_only_non_admin_401() {
        let _g = lock();
        let client = setup();
        seed("dar-nonadmin", false, None);
        let status = client
            .post("/post/admin/config/disable-read-only")
            .cookie(cookie("dar-nonadmin", false))
            .dispatch()
            .status();
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn disable_read_only_unauth_401() {
        let _g = lock();
        let client = setup();
        seed("dar-unauth", true, None);
        let status = client
            .post("/post/admin/config/disable-read-only")
            .dispatch()
            .status();
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn disable_read_only_works_no_body() {
        let _g = lock();
        let client = setup();
        seed("dar-nobody", true, None);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = true;
        let status = client
            .post("/post/admin/config/disable-read-only")
            .cookie(cookie("dar-nobody", true))
            .header(rocket::http::Header::new(
                "x-picasu-password",
                crate::tests::fixtures::test_password("dar-nobody"),
            ))
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn disable_read_only_missing_password_403() {
        let _g = lock();
        let client = setup();
        seed("dar-missing", true, None);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = true;
        let status = client
            .post("/post/admin/config/disable-read-only")
            .cookie(cookie("dar-missing", true))
            .dispatch()
            .status();
        assert_eq!(status, Status::Forbidden);
    }

    #[test]
    fn disable_read_only_wrong_password_403() {
        let _g = lock();
        let client = setup();
        seed("dar-wrong", true, None);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = true;
        let status = client
            .post("/post/admin/config/disable-read-only")
            .cookie(cookie("dar-wrong", true))
            .header(rocket::http::Header::new("x-picasu-password", "nope"))
            .dispatch()
            .status();
        assert_eq!(status, Status::Forbidden);
    }

    #[test]
    fn disable_read_only_survives_reload() {
        let _g = lock();
        let client = setup();
        seed("dar-reload", true, None);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = true;
        let status = client
            .post("/post/admin/config/disable-read-only")
            .cookie(cookie("dar-reload", true))
            .header(rocket::http::Header::new(
                "x-picasu-password",
                crate::tests::fixtures::test_password("dar-reload"),
            ))
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
        // Re-read config: mode stays off.
        let mode = crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .read()
            .expect("lock")
            .read_only_mode;
        assert!(!mode);
    }
}
