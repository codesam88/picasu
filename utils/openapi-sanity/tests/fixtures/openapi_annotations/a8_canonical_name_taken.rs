// A8's second branch: the canonical name for a guard class is already taken by
// another parameter in the same signature, so the binding cannot be exactly that
// name and must instead carry it as a word.
//
// Rocket binds a route's `?<name>` to a handler argument of the same name, which is
// how this arises: `GuardTimestamp` collides with `?<timestamp>` in every signature
// in `backend/src/router` that binds the class. The word-part match ignores
// underscores and case, so both spellings below satisfy the rule and `auth` —
// this repository's own mistake at `get/get_data.rs:191` and `:221` — does not.
//
// The fixture mixes the two outcomes on purpose, so that one render shows both:
// `taken_canonical_name` carries the class name and is silent, while
// `taken_canonical_name_not_carried` does not and is the finding. Without the
// second handler the taken-name branch would be indistinguishable from a blanket
// exemption.
//
// Every handler documents the status its guard rejects with, so C3 stays silent
// and the findings here are A8's alone.

/// Read one widget behind a timestamp token, named with the class name as a word.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = "Unauthorized"),
    )
)]
#[get("/get/a8-taken?<timestamp>")]
pub fn taken_canonical_name(
    guard_timestamp: GuardResult<GuardTimestamp>,
    timestamp: i64,
) -> AppResult<Json<Widget>> {
    let claims = guard_timestamp?;
    Ok(Json(Widget::from(claims)))
}

/// The same shape with the other spelling, which the word-part match also accepts.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = "Unauthorized"),
    )
)]
#[get("/get/a8-taken-suffix?<timestamp>")]
pub fn taken_canonical_name_suffix(
    timestamp_guard: GuardResult<GuardTimestamp>,
    timestamp: i64,
) -> AppResult<Json<Widget>> {
    let claims = timestamp_guard?;
    Ok(Json(Widget::from(claims)))
}

/// The finding: the name is taken and the binding does not carry the class name.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = "Unauthorized"),
    )
)]
#[get("/get/a8-taken-not-carried?<timestamp>")]
pub fn taken_canonical_name_not_carried(
    auth: GuardResult<GuardTimestamp>,
    timestamp: i64,
) -> AppResult<Json<Widget>> {
    let claims = auth?;
    Ok(Json(Widget::from(claims)))
}

/// A class name surrounded by other words still names the guard.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = "Unauthorized"),
    )
)]
#[get("/get/a8-taken-surrounded?<timestamp>")]
pub fn taken_canonical_name_surrounded(
    outer_inner_timestamp_guard: GuardResult<GuardTimestamp>,
    timestamp: i64,
) -> AppResult<Json<Widget>> {
    let claims = outer_inner_timestamp_guard?;
    Ok(Json(Widget::from(claims)))
}