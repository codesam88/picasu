use rocket::get;

// Clean: a public operation that cannot answer 401, and says so in the policy.
#[utoipa::path(get, path = "/login", tag = "pages")]
#[get("/login")]
pub async fn login() {}

// Drift: a public operation in no policy entry. Nothing about the source or the
// document is wrong, which is the point: a new route nobody classified looks
// exactly like a route someone reviewed and left open.
#[utoipa::path(get, path = "/setting", tag = "pages")]
#[get("/setting")]
pub async fn setting() {}

// Drift: a public operation that documents a 401 the policy does not account for.
#[utoipa::path(get, path = "/trashed", tag = "pages")]
#[get("/trashed")]
pub async fn trashed() {}
