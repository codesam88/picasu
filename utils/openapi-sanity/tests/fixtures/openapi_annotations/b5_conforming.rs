// The conforming counterpart of B5: every parameter the route binds carries a
// description — a query parameter, a path parameter, and a partial segment whose
// `..` is Rocket's marker rather than part of the name.

/// Read one asset, with both parameters described.
#[utoipa::path(
        tag = "assets",
        params(
            ("asset_id" = String, Path, description = "Asset to read"),
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

/// A partial-segment route: the `..` is Rocket's marker, not part of the name.
#[utoipa::path(
        tag = "serving",
        params(
            ("path" = String, Path, description = "Path under the asset root"),
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
