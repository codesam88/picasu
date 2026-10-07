// A9's failing fixture: top-level spellings utoipa's own grammar accepts that
// this crate's parser does not model, plus a body spelling it cannot read a
// schema from. `path.rs` in utoipa-gen matches `method`, `tags` and
// `context_path` alongside the spellings section A reads, so each of these is
// legal in a compiling annotation — and each reaches no rule unless the parser
// records what it skipped.

/// Get the album.
#[utoipa::path(
        tag = "albums",
        method(GET),
        tags(["albums"]),
        context_path = "/api",
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[get("/get/album")]
pub fn get_album() -> AppResult<String> {
    Ok(String::new())
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
#[post("/post/renew-hash-token", data = "<token_request>")]
pub fn renew_hash_token(token_request: Json<RenewHashToken>) -> AppResult<Json<String>> {
    let _ = token_request;
    Ok(Json(String::new()))
}
