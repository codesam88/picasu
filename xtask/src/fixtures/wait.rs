use rocket::local::blocking::Client;
use serde_json::Value;

use crate::fixtures::auth::auth_cookie;

/// Poll `GET /get/index/status` until the index reaches a terminal state
/// or the timeout expires.  Panics with a descriptive message on failure.
pub fn wait_for_album_index(client: &Client, timeout_ms: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let cookie = auth_cookie(client);

    loop {
        let resp = client
            .get("/get/index/status")
            .cookie(cookie.clone())
            .dispatch();

        let body: Value =
            serde_json::from_slice(&resp.into_bytes().expect("index status body"))
                .expect("valid index status JSON");

        let state = body["state"].as_str().unwrap_or("unknown");

        match state {
            "Completed" => return,
            "Failed" => {
                let detail = body["detail"].as_str().unwrap_or("(no detail)");
                panic!("Album index failed: {detail}");
            }
            "Canceled" => {
                panic!("Album index was canceled");
            }
            _ => {}
        }

        if std::time::Instant::now() > deadline {
            panic!(
                "Index did not complete within {timeout_ms} ms (state={state})",
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
