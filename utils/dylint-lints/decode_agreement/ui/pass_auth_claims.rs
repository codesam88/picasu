// Minimal stubs reproducing the resolved-type shapes the lint matches:
// - `request::FromRequest` trait (def path contains `request`)
// - `decode_typed` free function returning `Result<T, String>`
// - guard + claims types by model-assigned name

pub mod request {
    pub trait FromRequest {
        fn from_request(token: &str) -> Self;
    }
}

pub struct Claims(pub String);
pub struct ClaimsHash(pub String);
pub struct ClaimsTimestamp(pub u64);

pub struct GuardAuth;
pub struct GuardHash;

pub fn decode_typed<T>(token: &str) -> Result<T, String> {
    let _ = token;
    Err("stub".to_string())
}

// PASS: `GuardAuth` decodes its assigned `Claims`.
impl request::FromRequest for GuardAuth {
    fn from_request(token: &str) -> Self {
        let _claims: Claims = decode_typed(token).unwrap();
        GuardAuth
    }
}

fn main() {}
