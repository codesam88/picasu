// S10 conforming counterpart: a content write that carries the read-only gate
// and a manage-class operation confined to admins.

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

/// Write content behind the read-only gate.
#[utoipa::path(
    tag = "assets",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser", "GuardMutate"],
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
pub async fn write_widget(
    auth: GuardResult<GuardUser>,
    mutation: GuardResult<GuardMutate>,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Manage the server, admin only.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin", "GuardMutate"],
        "tokenTyp": ["session"],
        "admin": "admin",
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
    auth: GuardResult<GuardAdmin>,
    mutation: GuardResult<GuardMutate>,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Operational control that must stay available under read-only mode.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin"],
        "tokenTyp": ["session"],
        "admin": "admin",
        "shareScope": "n/a",
        "public": false,
        "effects": ["control"],
        "invariants": []
    }))),
    responses((status = 200, description = "Ok"))
)]
#[post("/post/control-widget")]
pub async fn control_widget(auth: GuardResult<GuardAdmin>) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
