use log::error;
use rocket::http::Status;
use rocket::post;

use tokio::task::spawn_blocking;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::config::AppConfig;
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::router::{AppResult, GuardResult};

/// Lift the read-only kill-switch.
///
/// Clears `readOnlyMode` and persists the change. Admin-only and deliberately
/// NOT gated by `GuardReadOnlyMode`: lifting the kill-switch must be callable
/// while it is on; the L3 confirm lives client-side.
///
/// Errors: 401 missing or invalid admin credentials — 500 config write failure.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    responses(
        (status = 200, description = "Read-only mode disabled"),
        (status = 401, response = Unauthorized),
        (status = 500, description = "Internal error"),
    )
)]
#[post("/post/admin/config/disable-read-only")]
pub async fn disable_read_only_handler(auth: GuardResult<GuardAuth>) -> AppResult<Status> {
    let _ = auth?;
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
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
    }
}
