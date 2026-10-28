use rocket::get;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::auth::users;
use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardUser;
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// The identity half of a self profile: read-only attributes owned by the
/// user store. `home` is `null` when unset.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct SelfProfileUser {
    pub user_id: String,
    pub admin: bool,
    pub home: Option<String>,
}

/// Reserved sub-object for future roaming preferences. Empty today: theme and
/// density stay client-only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct SelfProfileSettings {}

/// Single self resource returned by `GET /get/self/profile`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct SelfProfileResponse {
    pub user: SelfProfileUser,
    pub settings: SelfProfileSettings,
}

/// Read the caller's own identity from the user store.
///
/// Any logged-in user may call this; share tokens are rejected by `GuardUser`.
/// Reading is allowed while read-only mode is on.
///
/// Errors: 401 missing/invalid session or share token; 403 no enrolled
/// users; 404 unknown user; 500 storage failure.
#[utoipa::path(
    tag = "auth",
    security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardUser"],
            "tokenTyp": ["admin"],
            "admin": "either",
            "shareScope": "n-a",
            "readonly": false,
            "reauth": false,
            "public": false,
            "effects": [],
            "invariants": []
        }))),
    responses(
        (status = 200, description = "Self profile", body = SelfProfileResponse),
        (status = 401, response = Unauthorized),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal error"),
    )
)]
#[get("/get/self/profile")]
pub fn get_self_profile_handler(
    auth: GuardResult<GuardUser>,
) -> AppResult<Json<SelfProfileResponse>> {
    let auth = auth?;
    let id = match &auth.claims.role {
        crate::router::auth::Role::User { id, .. } => id.clone(),
        crate::router::auth::Role::Share(_) => {
            return Err(AppError::new(ErrorKind::Auth, "Share token not accepted"));
        }
    };
    let record = users::user_record(&id)
        .map_err(|e| AppError::from_err(ErrorKind::Database, e))?
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, format!("unknown user: {id}")))?;
    Ok(Json(SelfProfileResponse {
        user: SelfProfileUser {
            user_id: id,
            admin: record.admin,
            home: record.home,
        },
        settings: SelfProfileSettings {},
    }))
}

#[cfg(test)]
mod tests {
    use rocket::http::Status;

    use crate::tests::bootstrap::{cookie, lock, seed, setup};

    #[test]
    fn self_profile_returns_own_record() {
        let _g = lock();
        let client = setup();
        seed("sp-root", true, None);
        let resp = client
            .get("/get/self/profile")
            .cookie(cookie("sp-root", true))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("body");
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(v["user"]["userId"], "sp-root");
        assert_eq!(v["user"]["admin"], true);
        assert!(
            v["user"]["home"].is_null(),
            "home unset must be null: {body}"
        );
    }

    #[test]
    fn self_profile_non_admin_succeeds() {
        let _g = lock();
        let client = setup();
        seed("sp-nonadmin", false, None);
        let resp = client
            .get("/get/self/profile")
            .cookie(cookie("sp-nonadmin", false))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("body");
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(v["user"]["userId"], "sp-nonadmin");
        assert_eq!(v["user"]["admin"], false);
    }

    #[test]
    fn self_profile_unauthenticated_401() {
        let _g = lock();
        let client = setup();
        seed("sp-root2", true, None);
        let resp = client.get("/get/self/profile").dispatch();
        assert_eq!(resp.status(), Status::Unauthorized);
    }

    #[test]
    fn self_profile_works_while_read_only() {
        let _g = lock();
        let client = setup();
        seed("sp-ro", true, None);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = true;
        let resp = client
            .get("/get/self/profile")
            .cookie(cookie("sp-ro", true))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG")
            .write()
            .expect("lock")
            .read_only_mode = false;
    }

    #[test]
    fn self_profile_settings_is_empty_object() {
        let _g = lock();
        let client = setup();
        seed("sp-settings", true, None);
        let resp = client
            .get("/get/self/profile")
            .cookie(cookie("sp-settings", true))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("body");
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(v["settings"], serde_json::json!({}));
    }

    #[test]
    fn self_profile_uses_user_id_naming() {
        let _g = lock();
        let client = setup();
        seed("sp-naming", true, None);
        let resp = client
            .get("/get/self/profile")
            .cookie(cookie("sp-naming", true))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("body");
        assert!(!body.contains("username"), "must not use username: {body}");
        assert!(body.contains("userId"), "must use userId: {body}");
    }
}
