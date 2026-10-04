// A4 positive fixture: an annotated handler with nothing above it. `summary` and
// `description` are derived from the doc comment, so this operation reaches the
// generated reference with neither — and the document still looks complete.

#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn undocumented_widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}