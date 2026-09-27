use rocket::get;

// Under the excluded prefix: the public artifact omits the test-only probe
// surface on purpose, so this must not be reported as absent from the spec.
#[utoipa::path(get, path = "/get/test/record/{asset_id}", tag = "assets")]
#[get("/get/test/record/<asset_id>")]
pub fn probe_record() {}
