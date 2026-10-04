// P4: 418 is declared but nothing this handler can answer is 418 — not its
// success, not a guard, not a body kind, and not any AppError mapping.

type AppResult<T> = Result<T, AppError>;

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 418, description = "Teapot"),
    )
)]
#[get("/get/widget")]
pub fn get_widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
