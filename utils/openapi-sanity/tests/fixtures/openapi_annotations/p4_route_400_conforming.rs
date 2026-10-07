// P4 conforming counterpart: the route binds a query parameter, so Rocket can
// answer 400 on conversion failure before this non-fallible handler runs.

/// Count widgets.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 400, description = "Bad limit"),
    ),
    params(
        ("limit" = usize, Query, description = "Rows to count"),
    )
)]
#[get("/get/widget-count?<limit>")]
pub fn widget_count(limit: usize) -> Json<usize> {
    Json(limit)
}
