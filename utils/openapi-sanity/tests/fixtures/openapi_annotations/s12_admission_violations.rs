// S12 violations: a field no rule reads, an unknown fallback mechanism, an
// empty fallback, and a fallback on an operation with no guard path.
//
// `fallback` records the non-guard ways a handler may admit the caller. None
// of them is statically verifiable, so the rule checks the claims: the field
// set is closed, the vocabulary is fixed, an empty array is not a declaration,
// and a fallback refines a guard path rather than replacing one.

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

/// Carries a field the model does not define.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin", "GuardMutate"],
        "credentials": ["session"],
        "admin": "admin",
        "shareScope": "n-a",
        "control": "n-a",
        "effects": ["manage"],
        "invariants": ["AUTH-11", "AUTH-12"]
    }))),
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[post("/post/field-widget")]
pub async fn unknown_field(
    auth: GuardResult<GuardAdmin>,
    mutation: GuardResult<GuardMutate>,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Names an admission mechanism outside the vocabulary.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin", "GuardMutate"],
        "credentials": ["session"],
        "admin": "admin",
        "shareScope": "n-a",
        "fallback": ["invite"],
        "effects": ["manage"],
        "invariants": ["AUTH-11", "AUTH-12"]
    }))),
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[post("/post/mechanism-widget")]
pub async fn unknown_mechanism(
    auth: GuardResult<GuardAdmin>,
    mutation: GuardResult<GuardMutate>,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Declares a fallback that names nothing.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin", "GuardMutate"],
        "credentials": ["session"],
        "admin": "admin",
        "shareScope": "n-a",
        "fallback": [],
        "effects": ["manage"],
        "invariants": ["AUTH-11", "AUTH-12"]
    }))),
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[post("/post/empty-widget")]
pub async fn empty_fallback(
    auth: GuardResult<GuardAdmin>,
    mutation: GuardResult<GuardMutate>,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Declares a fallback on an operation everyone already reaches.
#[utoipa::path(
    tag = "config",
    security(()),
    extensions(("x-picasu-auth" = json!({
        "guards": [],
        "credentials": [],
        "admin": "none",
        "shareScope": "n-a",
        "fallback": ["allow-registration"],
        "effects": [],
        "invariants": []
    }))),
    responses((status = 200, description = "Ok"))
)]
#[post("/post/public-widget")]
pub async fn fallback_without_guards() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
