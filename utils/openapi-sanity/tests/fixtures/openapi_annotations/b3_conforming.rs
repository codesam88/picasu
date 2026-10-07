// The conforming counterpart of B3, and the three shapes the rule states it does
// not compare:
//
// - `create_dir_album` declares exactly the type the route parses — the happy
//   path;
// - `set_album_title` spells the same schema with a module path, which utoipa
//   publishes under the last segment, so it agrees;
// - `refresh_tokens` declares `Option<RenewHashToken>`, whose schema this tool
//   does not read;
// - `prefetch` declares `Value`, which is utoipa's "any body" and constrains
//   nothing the route could contradict;
// - `upload` binds a `Form<…>`, which has no schema type an annotation could
//   name, and declares its media type as B4 requires.

/// Create a directory album.
#[utoipa::path(
        tag = "albums",
        request_body = CreateDirAlbumData,
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/post/create_dir_album", format = "json", data = "<json_data>")]
pub fn create_dir_album(json_data: Json<CreateDirAlbumData>) -> AppResult<String> {
    let _ = json_data;
    Ok(String::new())
}

/// Set the album title.
#[utoipa::path(
        tag = "albums",
        request_body = crate::model::album::SetAlbumTitle,
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[put("/put/set_album_title", data = "<set_album_title>")]
pub fn set_album_title(set_album_title: Json<SetAlbumTitle>) -> AppResult<()> {
    let _ = set_album_title;
    Ok(())
}

/// Renew the hash token.
#[utoipa::path(
        tag = "auth",
        request_body = Option<RenewHashToken>,
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/post/renew-hash-token", format = "json", data = "<token_request>")]
pub fn renew_hash_token(token_request: Json<RenewHashToken>) -> AppResult<Json<String>> {
    let _ = token_request;
    Ok(Json(String::new()))
}

/// Prefetch a timeline page.
#[utoipa::path(
        tag = "timeline",
        params(
            ("locate" = Option<String>, Query, description = "Timeline position to start at"),
        ),
        request_body = serde_json::Value,
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/get/prefetch?<locate>", format = "json", data = "<query_data>")]
pub fn prefetch(query_data: Option<Json<Expression>>) -> AppResult<Json<PrefetchReturn>> {
    let _ = query_data;
    Ok(Json(PrefetchReturn::default()))
}

/// Upload files.
#[utoipa::path(
        tag = "upload",
        params(
            ("auto_rename" = Option<bool>, Query, description = "Rename an existing file instead of failing"),
        ),
        request_body(content_type = "multipart/form-data", content = Object),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/upload?<auto_rename>", data = "<form>")]
pub fn upload(form: Result<Form<UploadForm<'_>>, Errors<'_>>) -> AppResult<()> {
    let _ = form;
    Ok(())
}