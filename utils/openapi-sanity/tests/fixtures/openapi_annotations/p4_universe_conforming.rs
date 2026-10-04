// P4 conforming counterpart: a fallible handler declares 400 although no body
// literal raises InvalidInput — the error comes from a helper this tool does not
// follow, and the AppError mapping keeps the declaration inside the universe.

type AppResult<T> = Result<T, AppError>;

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 400, description = "Invalid input"),
    )
)]
#[get("/get/widget")]
pub fn get_widget() -> AppResult<Json<Widget>> {
    validate();
    Ok(Json(Widget))
}
