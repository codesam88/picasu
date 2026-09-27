use rocket::get;

// Drift: a data-API operation wearing the SPA page tag, which randomizes the
// reference grouping again.
#[utoipa::path(get, path = "/get/get-data", tag = "pages")]
#[get("/get/get-data?<timestamp>")]
pub async fn get_data() {}

// Drift: a tag outside the taxonomy. `metadata` is a subject the vocabulary
// deliberately does not have: its operations belong to `assets`.
#[utoipa::path(get, path = "/get/edit-tag", tag = "metadata")]
#[get("/get/edit-tag?<tag_id>")]
pub async fn edit_tag() {}

// Drift: what dropping `tag = "..."` from an annotation looks like — the operation
// still has one, and the document carries no `tags` for it.
#[utoipa::path(get, path = "/get/get-albums")]
#[get("/get/get-albums")]
pub async fn get_albums() {}

// Under the excluded prefix, and untagged with it: whether the public artifact
// carries this operation at all is the artifact's business, not the taxonomy's.
#[utoipa::path(get, path = "/get/test/record/{asset_id}", tag = "assets")]
#[get("/get/test/record/<asset_id>")]
pub fn probe_record() {}
