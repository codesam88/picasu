// FAIL: denial-as-forward — `Outcome::Forward` inside a `FromRequest` impl.
//
// The stub mirrors Rocket's layout: `Outcome` is *defined* outside `request`
// (real def path `rocket::outcome::Outcome`) and re-exported into it, so the
// lint — like on the real tree — matches the `Outcome::Forward` spelling,
// not the enum's definition site.

pub mod outcome {
    pub enum Outcome<S, E> {
        Success(S),
        Error((u16, E)),
        Forward(u16),
    }
}

pub mod request {
    pub trait FromRequest: Sized {
        type Error;
        fn from_request(token: &str) -> Outcome<Self, Self::Error>;
    }

    pub use crate::outcome::Outcome;
}

pub struct Guard;

impl request::FromRequest for Guard {
    type Error = String;
    fn from_request(token: &str) -> request::Outcome<Self, Self::Error> {
        if token.is_empty() {
            return request::Outcome::Forward(401);
        }
        request::Outcome::Success(Guard)
    }
}

fn main() {}
