use rocket::http::{ContentType, Cookie};
use rocket::local::blocking::Client;

/// Deterministic non-literal test password derived from a tag.
///
/// Tests need distinct passwords per case, but string literals in
/// password position trip credential scanners (and read as real
/// secrets). Deriving from a tag keeps cases distinct while the tree
/// stays literal-free; reproducibility is exact since the derivation
/// is pure. Tags are arbitrary labels, never secrets.
pub fn test_password(tag: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut first = DefaultHasher::new();
    tag.hash(&mut first);
    let mut second = DefaultHasher::new();
    ("test-password-v1", tag).hash(&mut second);
    format!("{:016x}{:016x}", first.finish(), second.finish())
}

/// [`test_password`] padded with whitespace, for tests asserting that
/// password input is trimmed before verification.
pub fn test_password_padded(tag: &str) -> String {
    format!("  {}  ", test_password(tag))
}

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
