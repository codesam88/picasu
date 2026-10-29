use rocket::get;
use rocket::serde::json::Json;
use serde::Serialize;

use crate::auth::users;
use crate::router::auth::bootstrap_open;
use crate::router::{AppError, AppResult, ErrorKind};

/// Captcha challenge descriptor, reserved for future brute-force throttling.
///
/// Never populated yet: the handler always answers `captcha: null`. Kept as
/// a named schema so a future challenge can extend it without reshaping the
/// enrollment response.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct CaptchaInfo {}

/// Anonymous enrollment state for boot routing.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct ServerInfo {
    pub enrolled: bool,
    pub enrollment_open: bool,
    pub captcha: Option<CaptchaInfo>,
}

/// `ServerInfo` JSON that must never be served stale.
///
/// A local responder (same manual-`Responder` shape as `FrontendResponse` in
/// `get_page.rs`): it delegates the body to `Json` and adjoins
/// `Cache-Control: no-store` via `raw_header`, the header-setting mechanism
/// of the cache fairing in `router::cache`.
pub struct NoStoreJson(Json<ServerInfo>);

impl<'r> rocket::response::Responder<'r, 'static> for NoStoreJson {
    fn respond_to(self, request: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
        rocket::response::Response::build_from(self.0.respond_to(request)?)
            .raw_header("Cache-Control", "no-store")
            .ok()
    }
}

/// Report anonymous enrollment state for boot routing.
///
/// Answers without credentials in every install state so the login and
/// register pages can route before any session exists: `enrolled` is true
/// once any user row exists, `enrollmentOpen` is true only on an empty
/// store with the bootstrap flag set, and `captcha` is always null for now.
///
/// Corner cases: the shape is fixed to exactly `enrolled`, `enrollmentOpen`
/// and `captcha` — no user rows or secrets are ever included. The response
/// carries `Cache-Control: no-store` because enrollment state flips on
/// first registration and must never be served stale.
///
/// Errors: 500 storage failure.
#[utoipa::path(
        tag = "auth",
        security(()),
        extensions(("x-picasu-auth" = json!({
            "guards": [],
            "credentials": [],
            "admin": "none",
            "shareScope": "n-a",
            "effects": [],
            "invariants": []
        }))),
        responses(
            (status = 200, description = "Enrollment state", body = ServerInfo),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/get/server-info")]
pub fn server_info_handler() -> AppResult<NoStoreJson> {
    let enrolled = users::user_count()
        .map(|count| count > 0)
        .map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
    let enrollment_open = !enrolled && bootstrap_open();
    Ok(NoStoreJson(Json(ServerInfo {
        enrolled,
        enrollment_open,
        captcha: None,
    })))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rocket::http::Status;
    use serde_json::{Value, json};

    use crate::tests::bootstrap::{lock, setup, write_config};

    #[test]
    fn anonymous_server_info_has_exactly_three_keys() {
        let _g = lock();
        let client = setup();
        let resp = client.get("/get/server-info").dispatch();
        assert_eq!(resp.status(), Status::Ok);
        assert_eq!(
            resp.headers().get_one("Cache-Control"),
            Some("no-store"),
            "enrollment state must never be served stale"
        );
        let body = resp.into_string().expect("body");
        let value: Value = serde_json::from_str(&body).expect("json body");
        let keys: BTreeSet<&str> = value
            .as_object()
            .expect("object body")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from(["captcha", "enrolled", "enrollmentOpen"]),
            "shape must stay exactly the three hand-built fields: {body}"
        );
    }

    #[test]
    fn empty_store_with_flag_open_reports_not_enrolled_but_open() {
        let _g = lock();
        let client = setup();
        write_config(&json!({ "bootstrap_admin": true }));
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        let resp = client.get("/get/server-info").dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body: Value =
            serde_json::from_str(&resp.into_string().expect("body")).expect("json body");
        assert_eq!(body["enrolled"], Value::Bool(false));
        assert_eq!(body["enrollmentOpen"], Value::Bool(true));
        assert!(body["captcha"].is_null(), "captcha is reserved: {body}");
    }

    #[test]
    fn empty_store_with_flag_closed_reports_not_enrolled_and_closed() {
        let _g = lock();
        let client = setup();
        write_config(&json!({ "bootstrap_admin": false }));
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        let resp = client.get("/get/server-info").dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body: Value =
            serde_json::from_str(&resp.into_string().expect("body")).expect("json body");
        assert_eq!(body["enrolled"], Value::Bool(false));
        assert_eq!(body["enrollmentOpen"], Value::Bool(false));
        assert!(body["captcha"].is_null(), "captcha is reserved: {body}");
    }

    #[test]
    fn non_empty_store_with_flag_open_reports_enrolled_and_closed() {
        let _g = lock();
        let client = setup();
        write_config(&json!({ "bootstrap_admin": true }));
        crate::auth::users::create_user("s7-enrolled-open", false).expect("seed user");
        let resp = client.get("/get/server-info").dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body: Value =
            serde_json::from_str(&resp.into_string().expect("body")).expect("json body");
        assert_eq!(body["enrolled"], Value::Bool(true));
        assert_eq!(body["enrollmentOpen"], Value::Bool(false));
        assert!(body["captcha"].is_null(), "captcha is reserved: {body}");
    }

    #[test]
    fn s9_pin_user_count_passwd_only_enrolled() {
        let _g = lock();
        let client = setup();
        write_config(&json!({ "bootstrap_admin": false }));
        // Construct a user that exists ONLY in the passwd store (no redb row).
        let path = crate::auth::users::user_store_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("auth dir");
        }
        crate::auth::user_store::provision(
            &path,
            "passwd-only",
            &crate::tests::fixtures::test_password("passwd-only"),
            false,
            None,
        )
        .expect("provision passwd-only");
        let resp = client.get("/get/server-info").dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body: Value =
            serde_json::from_str(&resp.into_string().expect("body")).expect("json body");
        assert_eq!(
            body["enrolled"],
            Value::Bool(true),
            "passwd-only user must count as enrolled"
        );
    }

    #[test]
    fn non_empty_store_with_flag_closed_reports_enrolled_and_closed() {
        let _g = lock();
        let client = setup();
        write_config(&json!({ "bootstrap_admin": false }));
        crate::auth::users::create_user("s7-enrolled-closed", false).expect("seed user");
        let resp = client.get("/get/server-info").dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body: Value =
            serde_json::from_str(&resp.into_string().expect("body")).expect("json body");
        assert_eq!(body["enrolled"], Value::Bool(true));
        assert_eq!(body["enrollmentOpen"], Value::Bool(false));
        assert!(body["captcha"].is_null(), "captcha is reserved: {body}");
    }
}
