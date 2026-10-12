// FAIL: the `GuardResult` binding is discarded with `let _ = auth;`.

pub mod router {
    #[derive(Debug)]
    pub struct AppError(pub String);
    pub type GuardResult<T> = Result<T, AppError>;
}

pub struct GuardAuth;

pub fn get_tags(auth: router::GuardResult<GuardAuth>) -> String {
    let _ = auth;
    String::from("tags")
}

fn main() {}
