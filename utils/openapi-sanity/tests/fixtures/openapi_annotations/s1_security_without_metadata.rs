// S1 reverse violation: `security(...)` without `x-picasu-auth` metadata —
// the document publishes requirements the model does not describe.

/// Fetch one widget.
///
/// The widget endpoint requires an admin session.
#[utoipa::path(
    tag = "assets",
    security(("cookie_jwt" = [])),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget(auth: GuardAdmin) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
