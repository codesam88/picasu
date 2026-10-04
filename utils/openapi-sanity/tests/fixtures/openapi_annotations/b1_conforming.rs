// The conforming counterpart of B1: every declared parameter is one the route
// binds, in both locations, and a route that declares no query part at all is
// not a finding either.

/// Read one asset, with a query parameter and a path parameter declared.
#[utoipa::path(
        tag = "assets",
        params(
            ("asset_id" = String, Path),
            ("timestamp" = Option<u64>, Query, description = "Snapshot to read"),
        ),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[get("/get/metadata/<asset_id>?<timestamp>")]
pub fn get_metadata(asset_id: String, timestamp: Option<u64>) -> AppResult<Json<Metadata>> {
    let _ = (asset_id, timestamp);
    Ok(Json(Metadata::default()))
}

/// A route with no query part, declaring only the path parameter it binds.
#[utoipa::path(
        tag = "assets",
        params(
            ("hash" = String, Path),
        ),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[get("/get/test/dupe-group/<hash>")]
pub fn dupe_group(hash: String) -> AppResult<()> {
    let _ = hash;
    Ok(())
}

/// A partial-segment route: the `..` is Rocket's marker, not part of the name.
#[utoipa::path(
        tag = "serving",
        params(
            ("path" = String, Path),
        ),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[get("/assets/<path..>")]
pub fn serve_file(path: String) -> AppResult<Vec<u8>> {
    let _ = path;
    Ok(Vec::new())
}