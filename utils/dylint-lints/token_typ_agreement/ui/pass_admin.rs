#![feature(register_tool)]
#![register_tool(utoipa)]
// PASS: GuardUser -> session matches tokenTyp.

pub mod request {
    pub trait FromRequest {}
}

pub struct GuardResult<T>(pub T);
pub struct GuardUser;

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser"],
        "tokenTyp": ["session"]
    })))
)]
pub fn handler(auth: GuardResult<GuardUser>) {}

fn main() {}
