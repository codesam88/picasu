// P3 fail-closed: a body kind the app-error map does not declare is a finding,
// not a status to guess at — the usual cause is a typo the compiler would also
// reject, and guessing would hide it.

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
        return Err(AppError::new(ErrorKind::Databse, "no such widget"));
    }
    Ok(())
}
