// S1 violation: `x-picasu-auth` metadata without `security(...)` — the model
// claims semantics the document does not publish.

/// Fetch one widget.
///
/// The widget endpoint answers anonymously.
#[utoipa::path(
    tag = "assets",
    extensions(("x-picasu-auth" = json!({
        "guards": [],
        "public": true
    }))),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
