// A1 conforming counterpart: the annotation says only what the route attribute
// cannot. Silence here is what keeps the rule from rejecting every annotation.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Create one widget.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[post("/post/widget")]
pub async fn create_widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}