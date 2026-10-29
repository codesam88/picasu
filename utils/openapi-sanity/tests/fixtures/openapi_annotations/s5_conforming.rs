// S5 conforming counterpart: `GuardMutate` is in the signature and 405 is
// declared, so the read-only rejection is documented.

pub struct GuardMutate;

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for GuardMutate {
    type Error = GuardError;

    async fn from_request(_req: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
        if crate::model::config::with_config(|c| c.read_only_mode) {
            return Outcome::Error((
                Status::MethodNotAllowed,
                AppError::new(ErrorKind::ReadOnlyMode, "Read-only mode is enabled"),
            ));
        }

        Outcome::Success(GuardMutate)
    }
}

type AppResult<T> = Result<T, AppError>;

/// Update the widget.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[put("/put/widget")]
pub fn update_widget(_read_only: GuardMutate) -> AppResult<()> {
    Ok(())
}
