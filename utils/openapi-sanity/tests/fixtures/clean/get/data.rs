use rocket::get;

#[utoipa::path(get, path = "/get/get-data", tag = "timeline")]
#[get("/get/get-data")]
pub async fn get_data() {}

#[utoipa::path(get, path = "/get/get-rows", tag = "timeline")]
#[get("/get/get-rows")]
pub async fn get_rows() {}

// The route URI carries a query part, which is documented per parameter rather
// than in the path. The two only agree once normalized.
#[utoipa::path(get, path = "/get/path-completion", tag = "config")]
#[get("/get/path-completion?<path>")]
pub async fn path_completion() {}

// A Rocket segment declaration against an OpenAPI path template.
#[utoipa::path(get, path = "/get/metadata/{asset_id}", tag = "assets")]
#[get("/get/metadata/<asset_id>")]
pub async fn get_metadata() {}
