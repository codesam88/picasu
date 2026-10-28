// S7 violation: a handler carrying a contributing guard but declaring no
// `security(...)` — the handler enforces credentials the document does not
// publish.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget(auth: GuardAuth) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
