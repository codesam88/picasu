use rocket::local::blocking::Client;
use serde_json::Value;

use super::auth::auth_cookie;

/// The state a scenario expects a `wait_index` step to reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexExpect {
    /// The scan finished with every matched file indexed.
    Completed,
    /// The scan finished in its `failed` state, i.e. every matched file failed.
    Failed,
}

/// Wait for the running album index to settle, then assert the state it reaches.
///
/// `Failed` is a result the harness can be asked for rather than a harness
/// error: an album whose every matched file is undecodable is a real outcome
/// (`did_every_matched_file_fail`), and a wait that only returned on
/// `completed` left such a scenario unable to express it. Settling on the other
/// state than the one asked for fails here, naming both what the index settled
/// as and what the scenario expected — so a scenario cannot pass by waiting for
/// a state it did not ask about.
///
/// `canceled` stays a panic under both expectations: it is neither outcome, and
/// nothing a scenario writes can ask for it. So does the timeout.
pub fn wait_for_album_index(client: &Client, timeout_ms: u64, expect: IndexExpect) {
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

        if matches!(state, "completed" | "failed") {
            if reached(state, expect) {
                return;
            }
            let detail = body["detail"].as_str().unwrap_or("(no detail)");
            panic!(
                "Album index settled as {state} (detail: {detail}), but the scenario expected \
                 it to be {}",
                label(expect)
            );
        }
        if state == "canceled" {
            panic!("Album index was canceled");
        }

        if std::time::Instant::now() > deadline {
            panic!(
                "Index did not settle as {} within {timeout_ms} ms (state={state})",
                label(expect)
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Whether a settled `state` is the one the scenario asked to wait for.
fn reached(state: &str, expect: IndexExpect) -> bool {
    matches!(
        (state, expect),
        ("completed", IndexExpect::Completed) | ("failed", IndexExpect::Failed)
    )
}

fn label(expect: IndexExpect) -> &'static str {
    match expect {
        IndexExpect::Completed => "completed",
        IndexExpect::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::{IndexExpect, reached};

    /// The two settled states are each what one expectation waits for and not
    /// the other, so neither spelling of `wait_index` can pass on the wrong
    /// outcome.
    #[test]
    fn only_the_expected_settled_state_counts_as_reached() {
        assert!(reached("completed", IndexExpect::Completed));
        assert!(reached("failed", IndexExpect::Failed));
        assert!(!reached("failed", IndexExpect::Completed));
        assert!(!reached("completed", IndexExpect::Failed));
    }
}
