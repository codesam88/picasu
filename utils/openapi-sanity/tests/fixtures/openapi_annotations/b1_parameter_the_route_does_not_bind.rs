// A B1 fixture: parameters the annotation declares that the route does not bind.
//
// Both spellings of the defect are here — a query parameter and a path parameter
// the route never reads — because they are read from two different parts of the
// route attribute, and a rule that only checked one of them would pass this
// fixture with the other case still open.
//
// A declared parameter the route *does* bind must not be reported, so the first
// handler declares one of each: `locate`, which the route binds, and `nope`,
// which nothing binds.

/// Prefetch a page for a timeline query.
#[utoipa::path(
        tag = "timeline",
        params(
            ("locate" = Option<String>, Query, description = "Timeline position to start at"),
            ("nope" = String, Query),
        ),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/get/prefetch?<locate>")]
pub fn prefetch(locate: Option<String>) -> AppResult<Json<PrefetchReturn>> {
    let _ = locate;
    Ok(Json(PrefetchReturn::default()))
}

/// Read one asset.
#[utoipa::path(
        tag = "assets",
        params(
            ("asset_id" = String, Path),
            ("album_id" = String, Path),
        ),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[get("/assets/<asset_id>")]
pub fn get_asset(asset_id: String) -> AppResult<Json<Asset>> {
    let _ = asset_id;
    Ok(Json(Asset::default()))
}