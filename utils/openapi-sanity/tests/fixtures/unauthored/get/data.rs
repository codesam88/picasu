use rocket::get;

// Clean: a deferred guard the handler propagates, behind the guard the policy
// names, with a 401 documented.
#[utoipa::path(get, path = "/get/get-data", tag = "timeline")]
#[get("/get/get-data")]
pub async fn get_data(guard_timestamp: GuardResult<GuardTimestamp>) {
    let _ = guard_timestamp?;
}

// Drift: the policy requires GuardAuth and the document documents the rejection,
// but the handler takes no guard at all.
#[utoipa::path(get, path = "/get/get-rows", tag = "timeline")]
#[get("/get/get-rows")]
pub async fn get_rows() {}

// Drift: the guard is there and its failure is dropped with `let _ = auth;`, so
// the handler serves an unauthorized caller. The one that has happened here.
#[utoipa::path(get, path = "/get/get-scroll-bar", tag = "timeline")]
#[get("/get/get-scroll-bar")]
pub async fn get_scroll_bar(auth: GuardResult<GuardTimestamp>) {
    let _ = auth;
}

// Drift: a guard the handler propagates, on an operation that documents no 401.
#[utoipa::path(get, path = "/get/get-tags", tag = "assets")]
#[get("/get/get-tags")]
pub async fn get_tags(auth: GuardResult<GuardAuth>) {
    let _ = auth?;
}
