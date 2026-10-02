// A5 positive fixture: the first paragraph runs to the second line. That
// paragraph is the operation's `summary`, which the reference renders as a
// heading — a newline inside a markdown heading splits it. This is the defect
// this repository's own document had.

/// Fetch one widget, resolved through the widget index by
/// its id.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget/<id>")]
pub async fn multi_line_summary() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}