// S12 conforming case: a guard-backed handler that names the non-guard paths
// it falls back to, and one whose guard path is the only admission.

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

/// Registration: admin, or an invite, or the open-registration knob.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin", "GuardMutate"],
        "credentials": ["session"],
        "admin": "admin",
        "shareScope": "n-a",
        "fallback": ["registration-invite", "allow-registration"],
        "effects": ["manage"],
        "invariants": ["AUTH-11", "AUTH-12"]
    }))),
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[post("/post/registration-widget")]
pub async fn registration_widget(
    auth: GuardResult<GuardAdmin>,
    mutation: GuardResult<GuardMutate>,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// An administrator-only operation: the guard path is the only admission.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin"],
        "credentials": ["session"],
        "admin": "admin",
        "shareScope": "n-a",
        "effects": ["read"],
        "invariants": ["AUTH-11"]
    }))),
    responses((status = 200, description = "Ok"))
)]
#[post("/post/admin-widget")]
pub async fn admin_widget(auth: GuardResult<GuardAdmin>) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
