// A6 conforming counterpart: no `operation_id`, so utoipa derives `widget` from
// the function name and every consumer of the name compares it.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}