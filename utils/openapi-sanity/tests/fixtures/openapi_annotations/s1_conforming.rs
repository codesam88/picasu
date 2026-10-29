// S1 conforming counterpart: `x-picasu-auth` metadata paired with an explicit
// `security(...)` declaration — the model and the document agree, so both the
// extension and the empty requirement are silent.

/// Fetch one widget.
///
/// The widget endpoint answers anonymously.
#[utoipa::path(
    tag = "assets",
    security(()),
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
