use rocket::get;

#[utoipa::path(get, path = "/login", tag = "pages")]
#[get("/login")]
pub async fn login() {}

#[utoipa::path(get, path = "/setting", tag = "pages")]
#[get("/setting")]
pub async fn setting() {}
