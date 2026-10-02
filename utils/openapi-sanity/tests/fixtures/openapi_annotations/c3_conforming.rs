// The conforming counterpart of C3: every guard class of `GUARD_CLASSES` beside
// the status it rejects with.
//
// - `every_credential_class` carries all six credential guards, all of which
//   reject with 401, and documents it once;
// - `read_only_route` carries the mode guard and documents 405 — the half of C3
//   that M2 used to assert on its own;
// - `both_statuses` carries a credential guard and the mode guard and documents
//   both;
// - `no_guard` carries no guard at all, so the rule asks nothing of it: an
//   operation that documents no 401 is not a finding unless a guard can produce
//   one.

/// Read one widget behind a token, a share or a presigned upload.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = Unauthorized),
    )
)]
#[get("/get/c3-every-credential-class")]
pub fn every_credential_class(
    auth: GuardResult<GuardAuth>,
    timestamp: GuardResult<GuardTimestamp>,
    share: GuardResult<GuardShare>,
    upload: GuardResult<GuardUpload>,
    hash: GuardResult<GuardHash>,
    hash_original: GuardResult<GuardHashOriginal>,
) -> AppResult<Json<Widget>> {
    let _ = auth?;
    let _ = timestamp?;
    let _ = share?;
    let _ = upload?;
    let _ = hash?;
    let _ = hash_original?;
    Ok(Json(Widget))
}

/// Rename a widget, refused while the build is read-only.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[put("/put/c3-read-only-mode")]
pub fn read_only_route(read_only_mode: GuardResult<GuardReadOnlyMode>) -> AppResult<Status> {
    let _ = read_only_mode?;
    Ok(Status::Ok)
}

/// Create a widget: guarded by a token and by the mode, documenting both.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = Unauthorized),
        (status = 405, description = "Read-only mode"),
    )
)]
#[post("/post/c3-both-statuses")]
pub fn both_statuses(
    _auth: GuardAuth,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
) -> AppResult<Status> {
    let _ = read_only_mode?;
    Ok(Status::Ok)
}

/// Read one widget with no guard, so there is no rejection status to document.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/c3-no-guard")]
pub fn no_guard() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}