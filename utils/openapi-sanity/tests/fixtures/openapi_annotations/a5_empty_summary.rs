// A5 fixture: a doc attribute exists but its first paragraph is empty, so utoipa
// has no summary text to publish.

///
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn empty_summary() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
