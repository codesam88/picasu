#![feature(register_tool)]
#![register_tool(utoipa)]
// FAIL: no guards but tokenTyp non-empty.

pub struct GuardResult<T>(pub T);

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": [],
        "tokenTyp": ["admin"]
    })))
)]
pub fn handler() {}

fn main() {}
