// A2 positive fixture: two ways of documenting nothing. utoipa invents no
// response, so both produce an operation that answers and says nothing.

/// Fetch one widget.
#[utoipa::path(tag = "assets")]
#[get("/get/widget")]
pub async fn no_responses() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Delete one widget.
#[utoipa::path(tag = "assets", responses())]
#[delete("/delete/widget")]
pub async fn empty_responses() -> AppResult<()> {
    Ok(())
}