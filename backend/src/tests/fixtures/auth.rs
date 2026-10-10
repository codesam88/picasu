use rocket::http::{ContentType, Cookie};
use rocket::local::blocking::Client;

pub fn auth_cookie(client: &Client) -> Cookie<'static> {
    // Open first-run mode accepts any parseable body, so post the login
    // shape: this keeps the fixture exercising the current API.
    let r = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(r#"{"userId":"admin","password":""}"#)
        .dispatch();
    let token = r.into_string().expect("token body");
    Cookie::new("jwt", token.trim_matches('"').to_owned())
}
