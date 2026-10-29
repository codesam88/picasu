#![feature(register_tool)]
#![register_tool(utoipa)]
// FAIL: no guards but credentials non-empty.

pub struct GuardResult<T>(pub T);

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": [],
        "credentials": ["session"]
    })))
)]
pub fn handler() {}

fn main() {}
