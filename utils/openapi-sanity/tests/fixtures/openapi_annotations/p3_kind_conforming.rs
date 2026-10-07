// P3 conforming counterpart: the status the body's ErrorKind maps to is
// declared.

type AppResult<T> = Result<T, AppError>;

/// Delete one widget.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Deleted"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Unknown widget"),
    ),
    params(
        ("id" = String, Path, description = "Widget to delete"),
    )
)]
#[delete("/delete/widget/<id>")]
pub fn delete_widget(id: String) -> AppResult<()> {
    if id.is_empty() {
        return Err(AppError::new(ErrorKind::NotFound, "no such widget"));
    }
    Ok(())
}
