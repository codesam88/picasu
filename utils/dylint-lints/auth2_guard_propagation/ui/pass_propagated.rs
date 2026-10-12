// PASS: `?` enforces the guard; `_` drops only the success value.
// (`?` desugars to a `match`, so the use is never a direct `let` discard.)

pub mod router {
    #[derive(Debug)]
    pub struct AppError(pub String);
    pub type GuardResult<T> = Result<T, AppError>;
}

pub struct GuardAuth;

pub fn get_tags(auth: router::GuardResult<GuardAuth>) -> router::GuardResult<String> {
    let _ = auth?;
    Ok(String::from("tags"))
}

fn main() {}
