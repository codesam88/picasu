use rocket::get;
use rocket::http::ContentType;
use rocket::serde::json::Json;

use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAdmin;
use crate::router::auth::GuardPresenter;
use serde::Serialize;

use crate::router::{AppResult, GuardResult};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
#[allow(clippy::struct_excessive_bools)]
pub struct ConfigResponse {
    pub upload_folder: String,
    pub max_upload_size: String,
    pub read_only_mode: bool,
    pub fs_notify_watcher: bool,
    pub normalize_upload_filenames: bool,
    pub validate_upload_content: bool,
    pub use_client_timestamp_info: bool,
}

/// Serve the client-visible server configuration.
///
/// Returns the values a client needs to drive the other operations:
/// `uploadFolder` (relative to the image library root), `maxUploadSize`,
/// `readOnlyMode`, `fsNotifyWatcher`, `normalizeUploadFilenames`,
/// `validateUploadContent` and `useClientTimestampInfo`.
///
/// Corner cases: the host image path is never served here — it is a
/// host-only setting (env/file) surfaced to admins through
/// `GET /get/admin/config`. The signing key is never exposed in any form.
/// `uploadFolder` is relative and visible to every admitted caller, so the
/// response carries no per-tier fields at all and is identical for every
/// credential that passes the gate.
///
/// Errors: 400 half-supplied share credentials or an id that is not an album —
/// 401 no valid admin or share credentials.
#[utoipa::path(
        tag = "config",
        security(("album_id" = [], "share_id" = []), ("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardPresenter"],
            "tokenTyp": ["session", "share"],
            "admin": "any",
            "shareScope": "n-a",
            "public": false,
            "effects": [],
            "invariants": []
        }))),
        responses(
            (status = 200, description = "Public configuration", body = ConfigResponse),
            (status = 400, description = "Invalid input"),
            (status = 404, description = "Not found"),
            (status = 401, response = Unauthorized),
        )
    )
]
#[get("/get/config")]
pub fn get_config_handler(auth: GuardResult<GuardPresenter>) -> AppResult<Json<ConfigResponse>> {
    let _ = auth?;
    let response = crate::model::config::with_config(|config| ConfigResponse {
        upload_folder: config.upload_folder.clone(),
        max_upload_size: config.max_upload_size.clone(),
        read_only_mode: config.read_only_mode,
        fs_notify_watcher: config.fs_notify_watcher,
        normalize_upload_filenames: config.normalize_upload_filenames,
        validate_upload_content: config.validate_upload_content,
        use_client_timestamp_info: config.use_client_timestamp_info,
    });
    Ok(Json(response))
}

/// Export the full server configuration as JSON.
///
/// Returns the complete serialized configuration with the signing key
/// redacted (`authKey` is removed, never exported), so the document stays
/// safe to store alongside backups. `webRoot` is the only other field never
/// serialized.
///
/// Errors: 400 invalid input — 401 no valid admin credentials.
#[utoipa::path(
        tag = "config",
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAdmin"],
            "tokenTyp": ["session"],
            "admin": "admin",
            "shareScope": "n-a",
            "public": false,
            "effects": [],
            "invariants": []
        }))),
        responses(
            (status = 200, description = "Exported configuration", body = crate::model::config::AppConfig, content_type = "application/json"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
        )
    )
]
#[get("/get/config/export")]
pub fn export_config_handler(auth: GuardResult<GuardAdmin>) -> AppResult<(ContentType, String)> {
    let _ = auth?;
    let value = crate::model::config::with_config(|config| {
        let mut value = serde_json::to_value(config).expect("configuration serializes to JSON");
        value
            .as_object_mut()
            .expect("configuration serializes to an object")
            .remove("authKey");
        value
    });
    let json = serde_json::to_string_pretty(&value).unwrap_or_default();
    Ok((ContentType::JSON, json))
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Status};

    use crate::tests::bootstrap::{cookie, lock, seed, setup};

    /// The config export must never carry the signing key: even with a key
    /// configured, the body redacts it while keeping the other fields.
    #[test]
    fn config_export_redacts_auth_key() {
        let _g = lock();
        let client = setup();
        seed("s4g-root", true, None);
        let admin = cookie("s4g-root", true);
        let status = client
            .post("/post/config/auth-key/reset")
            .cookie(admin.clone())
            .header(ContentType::JSON)
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok, "reset must establish a key first");
        let stored = crate::auth::secret_key::load_secret_key()
            .expect("load secrets file")
            .expect("key stored");

        // The reset invalidated the pre-reset session: mint a fresh one.
        let admin = cookie("s4g-root", true);
        let resp = client.get("/get/config/export").cookie(admin).dispatch();
        assert_eq!(resp.status(), Status::Ok);
        let body = resp.into_string().expect("export body");
        assert!(
            !body.contains("authKey"),
            "export must not name the signing key"
        );
        assert!(
            !body.contains(&stored),
            "export must not leak the key value"
        );
        assert!(
            body.contains("uploadFolder"),
            "export must still carry the other fields, got: {body}"
        );
    }
}
