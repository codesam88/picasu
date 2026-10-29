#![feature(register_tool)]
#![register_tool(utoipa)]
// PASS: GuardPresenter + GuardHash -> share + asset.

pub struct GuardResult<T>(pub T);
pub struct GuardPresenter;
pub struct GuardHash;

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardPresenter", "GuardHash"],
        "tokenTyp": ["session", "share", "asset"]
    })))
)]
pub fn handler(share: GuardResult<GuardPresenter>, hash: GuardResult<GuardHash>) {}

fn main() {}
