// PASS: `decode_typed` outside any `FromRequest` impl is not a guard
// decode and must stay silent.

pub mod request {
    pub trait FromRequest {
        fn from_request(token: &str) -> Self;
    }
}

pub struct Claims(pub String);

pub fn decode_typed<T>(token: &str) -> Result<T, String> {
    let _ = token;
    Err("stub".to_string())
}

pub fn helper(token: &str) -> Claims {
    decode_typed(token).unwrap()
}

fn main() {}
