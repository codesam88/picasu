use rocket::get;

// Drift: the other direction of the reserved tag. A subject tag on a page route
// is what the `pages` rule exists to prevent, since a page is not a subject.
#[utoipa::path(get, path = "/login", tag = "auth")]
#[get("/login")]
pub async fn login() {}

// Clean: a page route tagged the way the taxonomy requires.
#[utoipa::path(get, path = "/setting", tag = "pages")]
#[get("/setting")]
pub async fn setting() {}
