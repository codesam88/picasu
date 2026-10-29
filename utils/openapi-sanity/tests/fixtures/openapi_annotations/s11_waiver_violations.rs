// S11 violations: a waiver of a rule the operation does not declare, a waiver
// without a reason, and a waiver whose cause is gone.
//
// A waiver is the only sanctioned exemption mechanism, so each of these must be
// reported instead of silently excusing a rule.

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

/// Waives a rule the operation is not held to.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin"],
        "tokenTyp": ["session"],
        "admin": "admin",
        "shareScope": "n/a",
        "public": false,
        "effects": ["manage"],
        "invariants": ["AUTH-11"],
        "waivers": {
            "AUTH-12": "declared but never held"
        }
    }))),
    responses((status = 200, description = "Ok"))
)]
#[post("/post/widget")]
pub async fn undeclared_waiver(auth: GuardResult<GuardAdmin>) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Waives a rule without saying why.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin"],
        "tokenTyp": ["session"],
        "admin": "admin",
        "shareScope": "n/a",
        "public": false,
        "effects": ["manage"],
        "invariants": ["AUTH-11", "AUTH-12"],
        "waivers": {
            "AUTH-12": ""
        }
    }))),
    responses((status = 200, description = "Ok"))
)]
#[post("/post/other-widget")]
pub async fn unreasoned_waiver(auth: GuardResult<GuardAdmin>) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Waives a rule the handler already satisfies.
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
        "invariants": ["AUTH-11", "AUTH-12"],
        "waivers": {
            "AUTH-12": "the gate is already there"
        }
    }))),
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[post("/post/third-widget")]
pub async fn unused_waiver(
    auth: GuardResult<GuardAdmin>,
    mutation: GuardResult<GuardMutate>,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
