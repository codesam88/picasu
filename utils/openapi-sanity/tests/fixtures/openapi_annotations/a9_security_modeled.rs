// A9 conforming counterpart: `security(...)` is a modeled form — the checker
// reads it as the operation's security requirements and does not interpret
// the scheme names, so a handler declaring it produces no unsupported-form
// finding. Silence here pins that acceptance against the catch-all arm.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(("cookie_jwt" = [])),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Fetch another widget through the share headers or the admin cookie.
#[utoipa::path(
    tag = "assets",
    security(("album_id" = [], "share_id" = []), ("cookie_jwt" = [])),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/other-widget")]
pub async fn other_widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
