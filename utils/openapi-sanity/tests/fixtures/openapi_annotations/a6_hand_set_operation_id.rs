// A6 positive fixture: a hand-set `operation_id`. utoipa derives it from the
// function name, and every other name in the document is derived the same way or
// compared against the mount table — this one is compared by nothing.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    operation_id = "getWidget",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}