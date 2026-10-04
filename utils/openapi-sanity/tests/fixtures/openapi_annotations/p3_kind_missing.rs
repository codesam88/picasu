// P3: the body raises ErrorKind::NotFound, which http_status maps to 404; the
// annotation does not declare it.

type AppResult<T> = Result<T, AppError>;

/// Delete one widget.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Deleted"),
        (status = 401, description = "Unauthorized"),
    )
)]
#[delete("/delete/widget/<id>")]
pub fn delete_widget(id: String) -> AppResult<()> {
    if id.is_empty() {
        return Err(AppError::new(ErrorKind::NotFound, "no such widget"));
    }
    Ok(())
}
