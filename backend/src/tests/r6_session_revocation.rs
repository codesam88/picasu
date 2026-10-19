use rocket::http::{ContentType, Cookie, Status};
use serde_json::json;

use crate::tests::bootstrap::{lock, login, seed, setup};
use crate::tests::fixtures::test_password;

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
    // Both old cookies 401 on a GuardUser route
    assert_eq!(
        client
            .get("/get/sessions")
            .cookie(Cookie::new("jwt", tok_a))
            .dispatch()
            .status(),
        Status::Unauthorized
    );
    assert_eq!(
        client
            .get("/get/sessions")
            .cookie(Cookie::new("jwt", tok_b))
            .dispatch()
            .status(),
        Status::Unauthorized
    );
    // Re-login with new password works (use sessions inventory as GuardUser check, since /get/index/status is admin-only)
    let (status, _, tok_new) = login(&client, "r6-carol", &test_password("t-carol-new"));
    assert_eq!(status, Status::Ok);
    let tok_new = tok_new.expect("Set-Cookie");
    assert_eq!(
        client
            .get("/get/sessions")
            .cookie(Cookie::new("jwt", tok_new))
            .dispatch()
            .status(),
        Status::Ok
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
        client
            .get("/get/sessions")
            .cookie(Cookie::new("jwt", tok_dave))
            .dispatch()
            .status(),
        Status::Unauthorized
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

#[test]
fn inventory_lists_live_sessions() {
    let _g = lock();
    let client = setup();
    seed("r6-eve", true, Some(test_password("t-eve").as_str()));
    let (status, _, tok_a) = login(&client, "r6-eve", &test_password("t-eve"));
    assert_eq!(status, Status::Ok);
    let tok_a = tok_a.expect("Set-Cookie");
    let (status, _, tok_b) = login(&client, "r6-eve", &test_password("t-eve"));
    assert_eq!(status, Status::Ok);
    let tok_b = tok_b.expect("Set-Cookie");
    let val = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    let jti_a = crate::router::auth::decode_typed::<crate::router::auth::Claims>(&tok_a, &val)
        .unwrap()
        .jti;
    let jti_b = crate::router::auth::decode_typed::<crate::router::auth::Claims>(&tok_b, &val)
        .unwrap()
        .jti;
    // Inventory should have 2
    let resp = client
        .get("/get/sessions")
        .cookie(Cookie::new("jwt", tok_b.clone()))
        .dispatch();
    if resp.status() == Status::NotFound {
        panic!("GET /get/sessions not found — inventory not implemented");
    }
    assert_eq!(resp.status(), Status::Ok);
    let body = resp.into_string().expect("body");
    let listed: Vec<crate::auth::users::SessionRecord> = serde_json::from_str(&body).expect("json");
    assert_eq!(listed.len(), 2);
    // logout A
    let resp = client
        .post("/post/logout")
        .cookie(Cookie::new("jwt", tok_a))
        .dispatch();
    assert_eq!(resp.status(), Status::NoContent);
    let resp = client
        .get("/get/sessions")
        .cookie(Cookie::new("jwt", tok_b.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body = resp.into_string().expect("body");
    let listed: Vec<crate::auth::users::SessionRecord> = serde_json::from_str(&body).expect("json");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].jti, jti_b);
    assert_ne!(listed[0].jti, jti_a, "logged-out session must be gone");
}

#[test]
fn unauthenticated_inventory_is_401() {
    let _g = lock();
    let client = setup();
    seed("r6-frank", true, Some(test_password("t-frank").as_str()));
    let resp = client.get("/get/sessions").dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
}
