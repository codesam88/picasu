#![feature(register_tool)]
#![register_tool(utoipa)]
// FAIL: GuardUser -> admin but credentials says asset.

pub struct GuardResult<T>(pub T);
pub struct GuardUser;

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser"],
        "credentials": ["asset"]
    })))
)]
pub fn handler(auth: GuardResult<GuardUser>) {}

fn main() {}
