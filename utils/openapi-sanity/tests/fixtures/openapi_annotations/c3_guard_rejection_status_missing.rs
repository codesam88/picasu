// C3 fixture: a route carrying a guard whose annotation does not document the
// status that guard rejects with.
//
// - `share_without_401` can answer 401 because it binds `GuardShare`, and its
//   `responses(…)` says nothing of the sort;
// - `read_only_without_405` can answer 405 because it binds
//   `GuardReadOnlyMode` — a status a client cannot avoid and therefore has to be
//   told about — and its `responses(…)` lists only what the handler returns on
//   the happy path and on a bad request;
// - `one_missing_of_two` carries a credential guard **and** the mode guard and
//   documents one of the two statuses, which is the case that shows the rule
//   checks each class rather than stopping at the first.
//
// Every binding is named after its class, so the findings below are C3's alone.

/// Read one widget behind a share token.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 400, description = "Invalid input"),
    )
)]
#[get("/get/c3-share-without-401")]
pub fn share_without_401(share: GuardResult<GuardShare>) -> AppResult<Json<Widget>> {
    let _ = share?;
    Ok(Json(Widget))
}

/// Rename a widget, refused while the build is read-only.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 400, description = "Invalid input"),
        (status = 401, response = Unauthorized),
    )
)]
#[put("/put/c3-mode-without-405")]
pub fn read_only_without_405(
    _auth: GuardAuth,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
) -> AppResult<Status> {
    let _ = read_only_mode?;
    Ok(Status::Ok)
}

/// Create a widget, documenting only the token guard's status and not the mode's.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = Unauthorized),
    )
)]
#[post("/post/c3-one-missing-of-two")]
pub fn one_missing_of_two(
    _auth: GuardAuth,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
) -> AppResult<Status> {
    let _ = read_only_mode?;
    Ok(Status::Ok)
}