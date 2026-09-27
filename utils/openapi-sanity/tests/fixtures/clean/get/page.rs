use rocket::get;

#[utoipa::path(get, path = "/login", tag = "pages")]
#[get("/login")]
pub async fn login() {}

// Annotated and route-declaring but never registered: a function outside the
// route table is not part of the contract, so it must not be reported against
// the spec.
#[utoipa::path(get, path = "/setting", tag = "pages")]
#[get("/setting")]
pub async fn setting() {}
