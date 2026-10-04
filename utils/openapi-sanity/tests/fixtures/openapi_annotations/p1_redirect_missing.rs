// P1: a Redirect answers 302; the annotation claims 200, so P1 misses 302 and
// P4 finds 200 impossible.

/// Redirect to the login page.
#[utoipa::path(
    tag = "pages",
    responses((status = 200, description = "Ok"))
)]
#[get("/redirect-to-login")]
pub fn redirect_to_login() -> Redirect {
    Redirect::to(uri!("/login"))
}
