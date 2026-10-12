// FAIL (fail closed): `GuardShare` has no assigned decode target —
// it resolves via the share table, so any direct `decode_typed` warns.

pub mod request {
    pub trait FromRequest {
        fn from_request(token: &str) -> Self;
    }
}

pub struct Claims(pub String);

pub struct GuardShare;

pub fn decode_typed<T>(token: &str) -> Result<T, String> {
    let _ = token;
    Err("stub".to_string())
}

impl request::FromRequest for GuardShare {
    fn from_request(token: &str) -> Self {
        let _claims: Claims = decode_typed(token).unwrap();
        GuardShare
    }
}

fn main() {}
