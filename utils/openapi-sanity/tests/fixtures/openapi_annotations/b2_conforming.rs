// The conforming counterpart of B2: the declared type's optionality is the
// handler argument's optionality, in both directions, and a route with no
// declared parameters at all is silent.

/// Read a scroll bar.
#[utoipa::path(
        tag = "assets",
        params(
            ("timestamp" = Option<u64>, Query),
            ("limit" = u64, Query),
        ),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[get("/get/get-scroll-bar?<timestamp>&<limit>")]
pub fn get_scroll_bar(timestamp: Option<u64>, limit: u64) -> AppResult<Json<ScrollBar>> {
    let _ = (timestamp, limit);
    Ok(Json(ScrollBar::default()))
}

/// A route that declares no parameters, which rocket_extras derives from the signature.
#[utoipa::path(
        tag = "albums",
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[get("/get/get-albums")]
pub fn get_albums() -> AppResult<Json<Vec<Album>>> {
    Ok(Vec::new())
}