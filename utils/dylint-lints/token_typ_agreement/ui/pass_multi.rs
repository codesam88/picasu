#![feature(register_tool)]
#![register_tool(utoipa)]
// PASS: GuardUser + GuardReadOnlyMode -> admin.

pub struct GuardResult<T>(pub T);
pub struct GuardUser;
pub struct GuardReadOnlyMode;

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser", "GuardReadOnlyMode"],
        "tokenTyp": ["session"]
    })))
)]
pub fn handler(auth: GuardResult<GuardUser>, ro: GuardResult<GuardReadOnlyMode>) {}

fn main() {}
