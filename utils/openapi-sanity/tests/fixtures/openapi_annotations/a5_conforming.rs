// A5 conforming counterpart: the first paragraph is one line and the rest is a
// second one. Only the first paragraph is the summary, so wrapping the
// description across lines is fine — that is what paragraphs are for.

/// Fetch one widget.
///
/// Resolved through `WIDGET_BY_ID`, which is the only index that
/// carries the `id` of a widget. A 404 means the index has no such
/// entry, which is the answer a caller can act on.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok")),
    params(("id" = String, Path, description = "Widget to fetch")),
)]
#[get("/get/widget/<id>")]
pub async fn one_line_summary() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}