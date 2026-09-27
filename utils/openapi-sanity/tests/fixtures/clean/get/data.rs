use rocket::get;

// A deferred guard, propagated with `?`: Rocket hands the decision to the body,
// and the body acts on it.
#[utoipa::path(get, path = "/get/get-data", tag = "timeline")]
#[get("/get/get-data")]
pub async fn get_data(guard_timestamp: GuardResult<GuardTimestamp>) {
    let _ = guard_timestamp?;
}

// A guard behind a reference is still a direct guard: Rocket runs it before the
// body, so the body never mentions it.
#[utoipa::path(get, path = "/get/get-rows", tag = "timeline")]
#[get("/get/get-rows")]
pub async fn get_rows(_auth: &GuardAuth) {}

// The route URI carries a query part, which is documented per parameter rather
// than in the path. The two only agree once normalized. The guard is named by its
// path, as a handler reaching into another module would.
#[utoipa::path(get, path = "/get/path-completion", tag = "config")]
#[get("/get/path-completion?<path>")]
pub async fn path_completion(_auth: crate::router::auth::GuardAuth) {}

// A Rocket segment declaration against an OpenAPI path template.
#[utoipa::path(get, path = "/get/metadata/{asset_id}", tag = "assets")]
#[get("/get/metadata/<asset_id>")]
pub async fn get_metadata(_auth: GuardAuth) {}
