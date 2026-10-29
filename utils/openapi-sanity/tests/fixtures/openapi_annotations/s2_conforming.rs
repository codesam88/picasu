// S2 conforming counterpart: `GuardPresenter` implies the two-group declaration,
// and `GuardMutate` contributes no scheme, so it stays excluded.

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

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(("album_id" = [], "share_id" = []), ("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardPresenter", "GuardMutate"],
        "credentials": ["admin", "share"],
        "admin": "any",
        "shareScope": "own-album",
        "readonly": true,
        "reauth": false,
        "public": false,
        "effects": [],
        "invariants": []
    }))),
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[get("/get/widget")]
pub async fn widget(
    share: GuardPresenter,
    _read_only: GuardMutate,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
