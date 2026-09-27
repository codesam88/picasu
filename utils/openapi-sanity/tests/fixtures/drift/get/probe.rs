use rocket::get;

#[utoipa::path(get, path = "/get/test/record/{asset_id}", tag = "assets")]
#[get("/get/test/record/<asset_id>")]
pub fn probe_record() {}
