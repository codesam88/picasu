// PASS: denial is `Outcome::Error`. The `Outcome::Forward` in the plain
// function below is outside any `FromRequest` impl (like a Rocket route
// shim) and must stay silent — as must `Success`, which shares the enum.
//
// Layout mirrors Rocket: `Outcome` defined outside `request` (real def path
// `rocket::outcome::Outcome`), re-exported into it.

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
            return request::Outcome::Error((401, String::from("unauthorized")));
        }
        request::Outcome::Success(Guard)
    }
}

pub fn route_shim(token: &str) -> request::Outcome<Guard, String> {
    if token.is_empty() {
        return request::Outcome::Forward(401);
    }
    request::Outcome::Success(Guard)
}

fn main() {}
