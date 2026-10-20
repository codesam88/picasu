use rocket::http::Cookie;
use rocket::local::blocking::Client;

/// Deterministic non-literal test password derived from a tag.
///
/// Tests need distinct passwords per case, but string literals in
/// password position trip credential scanners (and read as real
/// secrets). Deriving from a tag keeps cases distinct while the tree
/// stays literal-free. Deterministic within a toolchain — std does not
/// pin `DefaultHasher`'s algorithm across versions, and nothing stored
/// (no snapshot, no fixture file) depends on the specific value. Tags
/// are arbitrary labels, never secrets.
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

/// Admin session cookie for tests that need an authenticated admin without
/// paying a login round-trip: ensures a real `admin` record exists (an empty
/// store holds no sessions by design) and mints a live session for it
/// locally. Tests that exercise enrollment or login itself must not use
/// this helper.
pub fn auth_cookie(_client: &Client) -> Cookie<'static> {
    if crate::auth::users::get_user("admin")
        .expect("read admin record")
        .is_none()
    {
        crate::auth::users::create_user("admin", true).expect("seed fixture admin");
    }
    crate::tests::bootstrap::cookie("admin", true)
}
