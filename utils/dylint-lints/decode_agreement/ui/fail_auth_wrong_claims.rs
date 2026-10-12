// FAIL: `GuardAuth` decodes `ClaimsHash` but the model assigns `Claims`.

pub mod request {
    pub trait FromRequest {
        fn from_request(token: &str) -> Self;
    }
}

pub struct Claims(pub String);
pub struct ClaimsHash(pub String);

pub struct GuardAuth;

pub fn decode_typed<T>(token: &str) -> Result<T, String> {
    let _ = token;
    Err("stub".to_string())
}

impl request::FromRequest for GuardAuth {
    fn from_request(token: &str) -> Self {
        let _claims: ClaimsHash = decode_typed(token).unwrap();
        GuardAuth
    }
}

fn main() {}
