#![feature(register_tool)]
#![register_tool(utoipa)]
// PASS: no guards -> empty tokenTyp.

pub struct GuardResult<T>(pub T);

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": [],
        "tokenTyp": []
    })))
)]
pub fn handler() {}

fn main() {}
