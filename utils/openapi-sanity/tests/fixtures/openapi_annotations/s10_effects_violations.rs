// S10 violations: an effect the guards do not support, and a manage-class
// operation the role model does not confine to admins.

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

/// Write content, but the signature carries no read-only gate.
#[utoipa::path(
    tag = "assets",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser"],
        "tokenTyp": ["session"],
        "admin": "any",
        "shareScope": "n/a",
        "public": false,
        "effects": ["write"],
        "invariants": []
    }))),
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[put("/put/widget")]
pub async fn write_widget(auth: GuardResult<GuardUser>) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Manage the server, but the route only needs any authenticated user.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser", "GuardMutate"],
        "tokenTyp": ["session"],
        "admin": "any",
        "shareScope": "n/a",
        "public": false,
        "effects": ["manage"],
        "invariants": []
    }))),
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[put("/put/manage-widget")]
pub async fn manage_widget(
    auth: GuardResult<GuardUser>,
    mutation: GuardResult<GuardMutate>,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
