use rocket::http::{ContentType, Cookie, Status};
use serde_json::json;

use crate::tests::bootstrap::{lock, login, seed, setup};
use crate::tests::fixtures::test_password;

/// Ask a `GuardUser` route about a token. The body names another user, so a
/// live non-admin token reaches the handler and is refused with 403 (it may
/// not change another user's password), while a dead token is rejected by
/// the guard with 401. Nothing is written either way.
fn probe(client: &rocket::local::blocking::Client, token: &str) -> Status {
    client
        .put("/put/users/password")
        .cookie(Cookie::new("jwt", token.to_string()))
        .header(ContentType::JSON)
        .body(
            json!({
                "userId": "r6-somebody-else",
                "newPassword": test_password("t-probe"),
            })
            .to_string(),
        )
        .dispatch()
        .status()
}

#[test]
fn logout_kills_only_its_session() {
    let _g = lock();
    let client = setup();
    seed("r6-alice", true, Some(test_password("t-alice").as_str()));
    let (status, _, tok_a) = login(&client, "r6-alice", &test_password("t-alice"));
    assert_eq!(status, Status::Ok);
    let tok_a = tok_a.expect("Set-Cookie");
    let (status, _, tok_b) = login(&client, "r6-alice", &test_password("t-alice"));
    assert_eq!(status, Status::Ok);
    let tok_b = tok_b.expect("Set-Cookie");
    // Both valid before logout
    assert_eq!(
        client
            .get("/get/index/status")
            .cookie(Cookie::new("jwt", tok_a.clone()))
            .dispatch()
            .status(),
        Status::Ok
    );
    assert_eq!(
        client
            .get("/get/index/status")
            .cookie(Cookie::new("jwt", tok_b.clone()))
            .dispatch()
            .status(),
        Status::Ok
    );
    // Logout A
    let resp = client
        .post("/post/logout")
        .cookie(Cookie::new("jwt", tok_a.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::NoContent);
    // A now 401, B still 200
    assert_eq!(
        client
            .get("/get/index/status")
            .cookie(Cookie::new("jwt", tok_a.clone()))
            .dispatch()
            .status(),
        Status::Unauthorized,
        "A's cookie must be 401 after logout"
    );
    assert_eq!(
        client
            .get("/get/index/status")
            .cookie(Cookie::new("jwt", tok_b.clone()))
            .dispatch()
            .status(),
        Status::Ok,
        "B's cookie must still be 200"
    );
}

#[test]
fn logout_with_garbage_still_204() {
    let _g = lock();
    let client = setup();
    seed("r6-bob", true, Some(test_password("t-bob").as_str()));
    let (status, _, tok) = login(&client, "r6-bob", &test_password("t-bob"));
    assert_eq!(status, Status::Ok);
    let tok = tok.expect("Set-Cookie");
    let resp = client
        .post("/post/logout")
        .cookie(Cookie::new("jwt", "garbage.token.here"))
        .dispatch();
    assert_eq!(resp.status(), Status::NoContent);
    let resp = client.post("/post/logout").dispatch();
    assert_eq!(resp.status(), Status::NoContent);
    // Original still valid when garbage logout did not kill it
    assert_eq!(
        client
            .get("/get/index/status")
            .cookie(Cookie::new("jwt", tok))
            .dispatch()
            .status(),
        Status::Ok
    );
}

#[test]
fn password_change_wipes_all_sessions() {
    let _g = lock();
    let client = setup();
    seed(
        "r6-carol",
        false,
        Some(test_password("t-carol-old").as_str()),
    );
    let (status, _, tok_a) = login(&client, "r6-carol", &test_password("t-carol-old"));
    assert_eq!(status, Status::Ok);
    let tok_a = tok_a.expect("Set-Cookie");
    let (status, _, tok_b) = login(&client, "r6-carol", &test_password("t-carol-old"));
    assert_eq!(status, Status::Ok);
    let tok_b = tok_b.expect("Set-Cookie");
    // Self password change
    let resp = client
        .put("/put/users/password")
        .cookie(Cookie::new("jwt", tok_a.clone()))
        .header(ContentType::JSON)
        .body(json!({"userId":"r6-carol","oldPassword": test_password("t-carol-old"), "newPassword": test_password("t-carol-new")}).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    // Both old cookies are refused: the password change bumped the user's
    // token version, so the tokens they carry are stale.
    assert_eq!(probe(&client, &tok_a), Status::Unauthorized);
    assert_eq!(probe(&client, &tok_b), Status::Unauthorized);
    // Re-login with the new password works.
    let (status, _, tok_new) = login(&client, "r6-carol", &test_password("t-carol-new"));
    assert_eq!(status, Status::Ok);
    let tok_new = tok_new.expect("Set-Cookie");
    assert_ne!(
        probe(&client, &tok_new),
        Status::Unauthorized,
        "the fresh token must be accepted"
    );
}

#[test]
fn admin_reset_wipes_target_sessions() {
    let _g = lock();
    let client = setup();
    seed(
        "r6-admin",
        true,
        Some(test_password("t-admin-old").as_str()),
    );
    seed("r6-dave", false, Some(test_password("t-dave-old").as_str()));
    let (status, _, tok_admin) = login(&client, "r6-admin", &test_password("t-admin-old"));
    assert_eq!(status, Status::Ok);
    let tok_admin = tok_admin.expect("Set-Cookie");
    let (status, _, tok_dave) = login(&client, "r6-dave", &test_password("t-dave-old"));
    assert_eq!(status, Status::Ok);
    let tok_dave = tok_dave.expect("Set-Cookie");
    let resp = client
        .put("/put/users/password")
        .cookie(Cookie::new("jwt", tok_admin.clone()))
        .header(ContentType::JSON)
        .body(json!({"userId":"r6-dave","newPassword": test_password("t-dave-new")}).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    assert_eq!(
        probe(&client, &tok_dave),
        Status::Unauthorized,
        "the target's tokens must die with the reset"
    );
    // admin still valid
    assert_eq!(
        client
            .get("/get/index/status")
            .cookie(Cookie::new("jwt", tok_admin))
            .dispatch()
            .status(),
        Status::Ok
    );
}
