#![feature(register_tool)]
#![register_tool(utoipa)]
// PASS: GuardUser -> session matches credentials.

pub mod request {
    pub trait FromRequest {}
}

pub struct GuardResult<T>(pub T);
pub struct GuardUser;

#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser"],
        "credentials": ["session"]
    })))
)]
pub fn handler(auth: GuardResult<GuardUser>) {}

fn main() {}
