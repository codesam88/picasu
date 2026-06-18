use rocket::http::{ContentType, Cookie};
use rocket::local::blocking::Client;

pub fn auth_cookie(client: &Client) -> Cookie<'static> {
    let r = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(r#""""#)
        .dispatch();
    let token = r.into_string().expect("token body");
    Cookie::new("jwt", token.trim_matches('"').to_owned())
}
