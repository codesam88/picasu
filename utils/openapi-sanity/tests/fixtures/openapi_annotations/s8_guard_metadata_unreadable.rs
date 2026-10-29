// S8 fail-closed: the extension exists but carries no readable `guards`
// array. Silence here would claim a check that never ran.

/// Fetch one widget.
///
/// The widget endpoint answers anonymously.
#[utoipa::path(
    tag = "assets",
    security(()),
    extensions(("x-picasu-auth" = json!({"public": true}))),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
