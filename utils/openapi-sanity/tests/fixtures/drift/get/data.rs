use rocket::get;

// Drift: the annotation documents a path the route does not serve. The spec has
// the renamed path, as it would after a regeneration, so the disagreement is
// reported once instead of also as spec drift.
#[utoipa::path(get, path = "/get/get-data-RENAMED", tag = "timeline")]
#[get("/get/get-data")]
pub async fn get_data() {}

// Drift: the annotation registers the operation under a verb the route does not
// serve.
#[utoipa::path(post, path = "/get/get-rows", tag = "timeline")]
#[get("/get/get-rows")]
pub async fn get_rows() {}

// Clean, and deliberately absent from the drift spec: an operation declared in
// source with no counterpart in the document.
#[utoipa::path(get, path = "/get/path-completion", tag = "config")]
#[get("/get/path-completion?<path>")]
pub async fn path_completion() {}

// Drift: registered in `routes![]` with no annotation of its own.
#[get("/get/metadata/<asset_id>")]
pub async fn get_metadata() {}
