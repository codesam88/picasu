// FAIL (fail closed): the guard binding is never used at all —
// an unenforced guard is silent at runtime, so zero uses still warn.

pub mod router {
    #[derive(Debug)]
    pub struct AppError(pub String);
    pub type GuardResult<T> = Result<T, AppError>;
}

pub struct GuardAdmin;

pub fn get_tags(auth: router::GuardResult<GuardAdmin>) -> String {
    String::from("tags")
}

fn main() {}
