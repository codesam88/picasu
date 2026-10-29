#![feature(register_tool)]
#![register_tool(utoipa)]
// PASS: GuardUser + GuardMutate -> admin.

pub struct GuardResult<T>(pub T);
pub struct GuardUser;
pub struct GuardMutate;

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser", "GuardMutate"],
        "tokenTyp": ["session"]
    })))
)]
pub fn handler(auth: GuardResult<GuardUser>, ro: GuardResult<GuardMutate>) {}

fn main() {}
