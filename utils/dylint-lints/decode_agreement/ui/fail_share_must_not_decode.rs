// FAIL (fail closed): `GuardPresenter` has no assigned decode target —
// it resolves via the share table, so any direct `decode_typed` warns.

pub mod request {
    pub trait FromRequest {
        fn from_request(token: &str) -> Self;
    }
}

pub struct ClaimsSession(pub String);

pub struct GuardPresenter;

pub fn decode_typed<T>(token: &str) -> Result<T, String> {
    let _ = token;
    Err("stub".to_string())
}

impl request::FromRequest for GuardPresenter {
    fn from_request(token: &str) -> Self {
        let _claims: ClaimsSession = decode_typed(token).unwrap();
        GuardPresenter
    }
}

fn main() {}
