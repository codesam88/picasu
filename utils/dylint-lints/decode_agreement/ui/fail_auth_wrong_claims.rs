// FAIL: `GuardAdmin` decodes `ClaimsAsset` but the model assigns `ClaimsSession`.

pub mod request {
    pub trait FromRequest {
        fn from_request(token: &str) -> Self;
    }
}

pub struct ClaimsSession(pub String);
pub struct ClaimsAsset(pub String);

pub struct GuardAdmin;

pub fn decode_typed<T>(token: &str) -> Result<T, String> {
    let _ = token;
    Err("stub".to_string())
}

impl request::FromRequest for GuardAdmin {
    fn from_request(token: &str) -> Self {
        let _claims: ClaimsAsset = decode_typed(token).unwrap();
        GuardAdmin
    }
}

fn main() {}
