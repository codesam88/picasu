// S2 conforming counterpart: `GuardShare` implies the two-group declaration,
// and `GuardReadOnlyMode` contributes no scheme, so it stays excluded.

pub struct GuardReadOnlyMode;

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for GuardReadOnlyMode {
    type Error = GuardError;

    async fn from_request(_req: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
        if crate::model::config::with_config(|c| c.read_only_mode) {
            return Outcome::Error((
                Status::MethodNotAllowed,
                AppError::new(ErrorKind::ReadOnlyMode, "Read-only mode is enabled"),
            ));
        }

        Outcome::Success(GuardReadOnlyMode)
    }
}

type AppResult<T> = Result<T, AppError>;

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(("album_id" = [], "share_id" = []), ("cookie_jwt" = [])),
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[get("/get/widget")]
pub async fn widget(
    share: GuardShare,
    _read_only: GuardReadOnlyMode,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
