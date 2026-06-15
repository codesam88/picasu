/// End-to-end API tests using Rocket's in-process blocking client.
///
/// These tests exercise the full handler stack (guards, fairings, DB) without
/// opening a real TCP port.  All global statics (DATA_PATH, APP_CONFIG, TREE)
/// are redirected to a shared temporary directory that is initialised once for
/// the lifetime of the test binary via TEST_ENV.
#[cfg(test)]
mod tests {
    use std::sync::{LazyLock, RwLock};

    use rocket::http::{ContentType, Cookie, Status};
    use rocket::local::blocking::Client;
    use tempfile::TempDir;

    use crate::public::constant::redb::DATA_TABLE;
    use crate::public::constant::storage::DATA_PATH;
    use crate::public::db::tree::TREE;
    use crate::public::structure::config::{APP_CONFIG, AppConfig};
    use crate::router::builder::build_rocket_with_config;

    // ---------------------------------------------------------------------------
    // One-time test environment

    struct TestEnv {
        // Keep the TempDir alive for the entire test run; dropping it would
        // delete the directory while redb still has the database open.
        _dir: TempDir,
    }

    static TEST_ENV: LazyLock<TestEnv> = LazyLock::new(|| {
        let dir = tempfile::tempdir().expect("create tempdir");

        // 1. Redirect DATA_PATH → tempdir so TREE / redb land there.
        DATA_PATH
            .set(dir.path().to_path_buf())
            .expect("DATA_PATH already set — ensure no earlier test initialised it");

        // 2. Seed APP_CONFIG with a no-password, no-auth-key config so
        //    GuardAuth always succeeds (try_jwt_cookie_auth short-circuits when
        //    password == None) and GuardReadOnlyMode passes (default = false).
        APP_CONFIG
            .set(RwLock::new(AppConfig::default()))
            .expect("APP_CONFIG already set");

        // 3. Create DATA_TABLE so read handlers don't get "table does not exist".
        //    In normal operation this happens on the first write (FlushTreeTask).
        {
            let write_txn = TREE.in_disk.begin_write().expect("begin write txn");
            write_txn.open_table(DATA_TABLE).expect("create DATA_TABLE");
            write_txn.commit().expect("commit DATA_TABLE creation");
        }

        TestEnv { _dir: dir }
    });

    // Build a fresh Rocket client for each test.  The global statics (TREE,
    // BATCH_COORDINATOR, etc.) are initialised lazily on first access and are
    // shared across clients within the same process — that is acceptable because
    // we treat the DB as append-only in Phase 1 tests.
    fn make_client() -> Client {
        let _ = &*TEST_ENV; // ensure one-time init runs first
        let config = AppConfig::default();
        let rocket = build_rocket_with_config(config);
        Client::tracked(rocket).expect("valid rocket instance")
    }

    // ---------------------------------------------------------------------------
    // Helpers

    /// Obtain a valid admin JWT cookie by calling POST /post/authenticate.
    /// Because APP_CONFIG has no password, any request body is accepted.
    fn get_auth_cookie(client: &Client) -> Cookie<'static> {
        let response = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(r#""""#)
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        // The authenticate handler returns the raw JWT string in the body.
        let token = response.into_string().expect("response body");
        // Strip the surrounding quotes that serde_json adds to a String value.
        let token = token.trim_matches('"').to_owned();
        Cookie::new("jwt", token)
    }

    // ---------------------------------------------------------------------------
    // Tests

    #[test]
    fn authenticate_without_password_returns_ok() {
        let client = make_client();
        let response = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(r#""""#)
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = response.into_string().unwrap_or_default();
        // Body is a JSON string — a non-empty JWT
        assert!(body.len() > 10, "expected a JWT token, got: {body:?}");
    }

    #[test]
    fn get_albums_without_password_configured_returns_ok() {
        // When no password is set in APP_CONFIG, GuardAuth short-circuits to
        // admin access — no token needed.  This is the expected no-auth mode.
        let client = make_client();
        let response = client.get("/get/get-albums").dispatch();
        assert_eq!(response.status(), Status::Ok);
    }

    #[test]
    fn get_albums_with_auth_returns_ok_and_list() {
        let client = make_client();
        let cookie = get_auth_cookie(&client);
        let response = client.get("/get/get-albums").cookie(cookie).dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = response.into_string().unwrap_or_default();
        // Must be a JSON array (may be empty on a fresh DB)
        assert!(body.starts_with('['), "expected JSON array, got: {body:?}");
    }

    #[test]
    fn create_empty_album_returns_album_id() {
        let client = make_client();
        let cookie = get_auth_cookie(&client);
        let response = client
            .post("/post/create_empty_album")
            .cookie(cookie)
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = response.into_string().unwrap_or_default();
        // Response is a plain string album ID (64 hex chars) — no JSON quotes
        // Album IDs are 64-char alphanumeric strings (lowercase + digits).
        assert_eq!(body.len(), 64, "expected 64-char album ID, got: {body:?}");
        assert!(
            body.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
            "album ID contains unexpected chars: {body:?}"
        );
    }

    #[test]
    fn created_album_appears_in_get_albums() {
        let client = make_client();
        let cookie = get_auth_cookie(&client);

        // Create an album
        let create_response = client
            .post("/post/create_empty_album")
            .cookie(cookie.clone())
            .dispatch();
        assert_eq!(create_response.status(), Status::Ok);
        let album_id = create_response.into_string().expect("album id");

        // List albums and check the new one is present
        let list_response = client.get("/get/get-albums").cookie(cookie).dispatch();
        assert_eq!(list_response.status(), Status::Ok);
        let body = list_response.into_string().unwrap_or_default();
        assert!(
            body.contains(&album_id),
            "album {album_id} not found in albums list: {body}"
        );
    }

    #[test]
    fn get_tags_with_auth_returns_empty_list() {
        let client = make_client();
        let cookie = get_auth_cookie(&client);
        let response = client.get("/get/get-tags").cookie(cookie).dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = response.into_string().unwrap_or_default();
        // On a fresh DB with no media, tag list is an empty JSON array
        assert_eq!(body.trim(), "[]", "expected empty tag list, got: {body:?}");
    }
}
