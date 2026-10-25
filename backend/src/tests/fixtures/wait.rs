use rocket::local::blocking::Client;
use serde_json::Value;

use super::auth::auth_cookie;

pub fn wait_for_album_index(client: &Client, timeout_ms: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let cookie = auth_cookie(client);

    loop {
        let resp = client
            .get("/get/index/status")
            .cookie(cookie.clone())
            .dispatch();

        let body: Value = serde_json::from_slice(&resp.into_bytes().expect("index status body"))
            .expect("valid index status JSON");

        let state = body["state"].as_str().unwrap_or("unknown");

        match state {
            "completed" => return,
            "failed" => {
                let detail = body["detail"].as_str().unwrap_or("(no detail)");
                panic!("Album index failed: {detail}");
            }
            "canceled" => {
                panic!("Album index was canceled");
            }
            _ => {}
        }

        assert!(
            std::time::Instant::now() <= deadline,
            "Index did not complete within {timeout_ms} ms (state={state})"
        );

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
