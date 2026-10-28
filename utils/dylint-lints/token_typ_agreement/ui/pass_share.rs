#![feature(register_tool)]
#![register_tool(utoipa)]
// PASS: GuardShare + GuardHash -> share + asset.

pub struct GuardResult<T>(pub T);
pub struct GuardShare;
pub struct GuardHash;

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardShare", "GuardHash"],
        "tokenTyp": ["admin", "share", "asset"]
    })))
)]
pub fn handler(share: GuardResult<GuardShare>, hash: GuardResult<GuardHash>) {}

fn main() {}
