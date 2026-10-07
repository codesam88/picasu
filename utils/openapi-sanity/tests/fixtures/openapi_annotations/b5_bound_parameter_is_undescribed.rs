// A B5 fixture: bound parameters the annotation does not describe.
//
// Both spellings of the defect, because they anchor differently: a parameter
// with no `params(…)` entry is reported at the route attribute, one the
// annotation declares but leaves undescribed at its own line.

/// Prefetch a page for a timeline query.
#[utoipa::path(
        tag = "timeline",
        params(
            ("locate" = Option<String>, Query),
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
