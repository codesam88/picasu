// The conforming counterpart of B4: form routes that name the media type, in both
// spellings utoipa accepts for it.
//
// - `upload` uses `request_body(content_type = "…", content = Object)`;
// - `import_config` uses the group form `request_body(content((Object =
//   "multipart/form-data")))`, where the media type is written inside `content`.
//
// A JSON route that names no media type is not a finding either, because
// utoipa's default for a named non-primitive type is application/json and that is
// the correct media type for it.

/// Upload files.
#[utoipa::path(
        tag = "upload",
        request_body(content_type = "multipart/form-data", content = Object),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/upload", data = "<form>")]
pub fn upload(form: Result<Form<UploadForm<'_>>, Errors<'_>>) -> AppResult<()> {
    let _ = form;
    Ok(())
}

/// Import a configuration.
#[utoipa::path(
        tag = "config",
        request_body(content((Object = "multipart/form-data"))),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/post/config/import", data = "<form>")]
pub fn import_config(form: Result<Form<ConfigImportForm<'_>>, Errors<'_>>) -> AppResult<Status> {
    let _ = form;
    Ok(Status::Ok)
}

/// Authenticate.
#[utoipa::path(
        tag = "auth",
        request_body = String,
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/post/authenticate", data = "<password>")]
pub fn authenticate(password: Json<String>) -> AppResult<Json<String>> {
    let _ = password;
    Ok(Json(String::new()))
}