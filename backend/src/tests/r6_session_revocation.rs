use rocket::http::{ContentType, Cookie, Status};
use serde_json::json;

use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD, make_client, reset_backend_state};
use crate::tests::fixtures::test_password;

fn lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn setup() -> rocket::local::blocking::Client {
    let _ = &*TEST_ENV;
    reset_backend_state();
    make_client()
}
fn create_user(id: &str, admin: bool, password: &str) {
    crate::auth::users::create_user(id, admin).expect("create user");
    let path = crate::auth::users::passwd_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("auth dir");
    }
    let mut store = crate::auth::password::PasswdFile::load(&path).expect("load passwd");
    store.set_password(id, password).expect("set password");
}
fn login(client: &rocket::local::blocking::Client, id: &str, pw: &str) -> String {
    let resp = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(json!({ "userId": id, "password": pw }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let set_cookie = resp.headers().get_one("Set-Cookie").expect("Set-Cookie");
    let pair = set_cookie.split(';').next().expect("cookie pair");
    pair.strip_prefix("jwt=").expect("jwt").to_string()
}

#[test]
fn logout_kills_only_its_session() {
    let _g = lock();
    let client = setup();
    create_user("r6-alice", true, &test_password("t-alice"));
    let tok_a = login(&client, "r6-alice", &test_password("t-alice"));
    let tok_b = login(&client, "r6-alice", &test_password("t-alice"));
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
    create_user("r6-bob", true, &test_password("t-bob"));
    let tok = login(&client, "r6-bob", &test_password("t-bob"));
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
    create_user("r6-carol", false, &test_password("t-carol-old"));
    let tok_a = login(&client, "r6-carol", &test_password("t-carol-old"));
    let tok_b = login(&client, "r6-carol", &test_password("t-carol-old"));
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
    let tok_new = login(&client, "r6-carol", &test_password("t-carol-new"));
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
    create_user("r6-admin", true, &test_password("t-admin-old"));
    create_user("r6-dave", false, &test_password("t-dave-old"));
    let tok_admin = login(&client, "r6-admin", &test_password("t-admin-old"));
    let tok_dave = login(&client, "r6-dave", &test_password("t-dave-old"));
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
    create_user("r6-eve", true, &test_password("t-eve"));
    let tok_a = login(&client, "r6-eve", &test_password("t-eve"));
    let tok_b = login(&client, "r6-eve", &test_password("t-eve"));
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
    create_user("r6-frank", true, &test_password("t-frank"));
    let resp = client.get("/get/sessions").dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
}
