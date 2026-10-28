// S2 violation: `security(...)` declares a scheme that does not match the
// guard in the signature — `GuardAuth` implies `cookie_jwt`, not
// `bearer_token`.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(("bearer_token" = [])),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget(auth: GuardAuth) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
