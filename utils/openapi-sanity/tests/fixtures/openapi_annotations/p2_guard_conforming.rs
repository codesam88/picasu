// P2 conforming counterpart: the guard's literal outcome status is declared.

pub struct GuardLocked;

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for GuardLocked {
    type Error = GuardError;

    async fn from_request(req: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
        if is_locked(req) {
            return Outcome::Error((
                Status::MethodNotAllowed,
                AppError::new(ErrorKind::ReadOnlyMode, "locked"),
            ));
        }
        Outcome::Success(GuardLocked)
    }
}

type AppResult<T> = Result<T, AppError>;

/// Update the widget.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, description = "Unauthorized"),
        (status = 405, description = "Locked"),
    )
)]
#[put("/put/widget")]
pub fn update_widget(_locked: GuardLocked) -> AppResult<()> {
    Ok(())
}
