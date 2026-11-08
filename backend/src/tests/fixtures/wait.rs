use rocket::local::blocking::Client;
use serde_json::Value;

use super::auth::auth_cookie;

/// Timeout (ms) for waiting on an album index to reach a terminal state.
///
/// Overridable at runtime via the `PICASU_TEST_INDEX_TIMEOUT_MS` environment
/// variable; an unset or unparseable value falls back to 90 seconds. Parsed
/// at call time so tuning does not require a rebuild (consistent with the
/// `PICASU_WEAK_PWHASH` runtime-env precedent).
pub fn index_timeout_ms() -> u64 {
    std::env::var("PICASU_TEST_INDEX_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(90_000)
}

pub fn wait_for_album_index(client: &Client, timeout_ms: u64) {
    let started = std::time::Instant::now();
    let deadline = started + std::time::Duration::from_millis(timeout_ms);
    let cookie = auth_cookie(client);

    loop {
        let resp = client
            .get("/get/index/status")
            .cookie(cookie.clone())
            .dispatch();

        let bytes = resp.into_bytes().expect("index status body");
        let body: Value = serde_json::from_slice(&bytes).expect("valid index status JSON");

        let state = body["state"].as_str().unwrap_or("unknown");
        let body_text = String::from_utf8_lossy(&bytes);
        let body_snippet: String = if body_text.chars().count() > 500 {
            let truncated: String = body_text.chars().take(500).collect();
            format!("{truncated}…")
        } else {
            body_text.into_owned()
        };

        match state {
            "completed" => return,
            "failed" => {
                let detail = body["detail"].as_str().unwrap_or("(no detail)");
                panic!(
                    "Album index failed: {detail} (elapsed {} ms)\nstatus: {body_snippet}",
                    started.elapsed().as_millis()
                );
            }
            "canceled" => {
                panic!(
                    "Album index was canceled (elapsed {} ms)\nstatus: {body_snippet}",
                    started.elapsed().as_millis()
                );
            }
            _ => {}
        }

        assert!(
            std::time::Instant::now() <= deadline,
            "Index did not complete within {timeout_ms} ms (state={state}, elapsed {} ms)\nstatus: {body_snippet}",
            started.elapsed().as_millis()
        );

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
