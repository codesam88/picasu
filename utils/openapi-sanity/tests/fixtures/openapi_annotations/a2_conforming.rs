// A2 conforming counterpart: one declared response is enough. The rule asks that
// the operation document what it can answer, not that it document every status.

/// Fetch one widget.
#[utoipa::path(tag = "assets", responses((status = 200, description = "Ok")))]
#[get("/get/widget")]
pub async fn one_response() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Delete one widget.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Deleted"),
        (status = 404, description = "Unknown widget"),
    )
)]
#[delete("/delete/widget")]
pub async fn two_responses() -> AppResult<()> {
    Ok(())
}

type AppResult<T> = Result<T, AppError>;