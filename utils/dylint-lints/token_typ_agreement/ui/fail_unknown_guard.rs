#![feature(register_tool)]
#![register_tool(utoipa)]
// FAIL (fail closed): unknown guard.

pub struct GuardResult<T>(pub T);
pub struct GuardFoo;

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardFoo"],
        "tokenTyp": []
    })))
)]
pub fn handler(auth: GuardResult<GuardFoo>) {}

fn main() {}
