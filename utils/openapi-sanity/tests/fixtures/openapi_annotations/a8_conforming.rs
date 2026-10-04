// The conforming counterpart of A8: every guard class of `GUARD_CLASSES` bound
// under the name that class expects, in the three spellings the tree uses.
//
// - `auth`/`guard_timestamp`/`share`/`upload`/`hash`/`hash_original`/`read_only_mode`
//   are the expected names, for a fallible guard and for a plain one;
// - `_share` is a plain guard whose value the handler never uses, which is the
//   tree's spelling and the one the leading underscore is allowed for;
// - `read_only_mode` next to `_auth` is the mutating-route shape the tree uses
//   throughout: a discarded token guard and a propagated mode guard.
//

/// Read one widget behind a token, a share or a presigned upload.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = Unauthorized),
    )
)]
#[get("/get/a8-every-class")]
pub fn every_class(
    auth: GuardResult<GuardAuth>,
    guard_timestamp: GuardResult<GuardTimestamp>,
    share: GuardResult<GuardShare>,
    upload: GuardResult<GuardUpload>,
    hash: GuardResult<GuardHash>,
    hash_original: GuardResult<GuardHashOriginal>,
) -> AppResult<Json<Widget>> {
    let _ = auth?;
    let _ = guard_timestamp?;
    let _ = share?;
    let _ = upload?;
    let _ = hash?;
    let _ = hash_original?;
    Ok(Json(Widget))
}

/// The mutating shape: an ignored token guard and a propagated mode guard.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = Unauthorized),
        (status = 405, description = "Read-only mode"),
    )
)]
#[put("/put/a8-read-only-mode")]
pub fn read_only_route(
    _auth: GuardAuth,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
) -> AppResult<Status> {
    let _ = read_only_mode?;
    Ok(Status::Ok)
}

/// A plain guard whose value the handler never uses carries the underscore.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = Unauthorized),
    )
)]
#[get("/get/a8-discarded-share")]
pub fn discarded_share(_share: GuardShare) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// A guard type outside `GUARD_CLASSES` is out of scope, not guessed at.
#[utoipa::path(
    tag = "auth",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/a8-unclassified-guard")]
pub fn unclassified(auth: TimestampGuardModified) -> AppResult<Json<Widget>> {
    let _ = auth;
    Ok(Json(Widget))
}