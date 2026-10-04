// P1: a Redirect answers the status its constructor names — `Redirect::to` is
// 303 and `Redirect::found` is 302. The first handler declares 200, so P1
// misses 303 and P4 finds 200 impossible; the second declares its constructor's
// status and is silent.

/// Redirect to the login page.
#[utoipa::path(
    tag = "pages",
    responses((status = 200, description = "Ok"))
)]
#[get("/redirect-to-login")]
pub fn redirect_to_login() -> Redirect {
    Redirect::to(uri!("/login"))
}

/// Send a found redirect to the album.
#[utoipa::path(
    tag = "albums",
    responses((status = 302, description = "Found"))
)]
#[get("/redirect-to-album")]
pub fn redirect_to_album() -> Redirect {
    Redirect::found(uri!("/albums"))
}
