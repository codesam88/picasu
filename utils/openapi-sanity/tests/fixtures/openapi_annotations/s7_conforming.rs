// S7 conforming counterpart: an explicitly public handler with `security(())`
// and no guards, plus a guarded handler whose `security(...)` matches.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(()),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn public_widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(("cookie_jwt" = [])),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget2")]
pub async fn guarded_widget(auth: GuardAuth) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
