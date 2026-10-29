// S8 conforming counterpart: the declared guard set is exactly the handler's
// signature, and a read-only gate counts as a signature guard.

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
///
/// The widget endpoint accepts any authenticated user and mutates it.
#[utoipa::path(
    tag = "assets",
    security(("album_id" = [], "share_id" = []), ("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardPresenter", "GuardMutate"],
        "credentials": ["session", "share"],
        "admin": "any",
        "shareScope": "album",
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
    presenter: GuardResult<GuardPresenter>,
    mutation: GuardResult<GuardMutate>,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
