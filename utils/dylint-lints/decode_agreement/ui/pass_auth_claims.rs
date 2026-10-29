// Minimal stubs reproducing the resolved-type shapes the lint matches:
// - `request::FromRequest` trait (def path contains `request`)
// - `decode_typed` free function returning `Result<T, String>`
// - guard + claims types by model-assigned name

pub mod request {
    pub trait FromRequest {
        fn from_request(token: &str) -> Self;
    }
}

pub struct ClaimsSession(pub String);
pub struct ClaimsAsset(pub String);
pub struct ClaimsSnapshot(pub u64);

pub struct GuardAdmin;
pub struct GuardHash;

pub fn decode_typed<T>(token: &str) -> Result<T, String> {
    let _ = token;
    Err("stub".to_string())
}

// PASS: `GuardAdmin` decodes its assigned `ClaimsSession`.
impl request::FromRequest for GuardAdmin {
    fn from_request(token: &str) -> Self {
        let _claims: ClaimsSession = decode_typed(token).unwrap();
        GuardAdmin
    }
}

fn main() {}
