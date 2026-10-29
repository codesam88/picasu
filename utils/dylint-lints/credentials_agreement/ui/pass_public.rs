#![feature(register_tool)]
#![register_tool(utoipa)]
// PASS: no guards -> empty credentials.

pub struct GuardResult<T>(pub T);

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": [],
        "credentials": []
    })))
)]
pub fn handler() {}

fn main() {}
