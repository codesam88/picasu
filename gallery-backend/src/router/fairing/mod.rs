use std::sync::LazyLock;

use jsonwebtoken::{Algorithm, Validation};
use rocket::Route;

pub mod auth_utils;
pub mod cache_control_fairing;
pub mod guard_auth;
pub mod guard_hash;
pub mod guard_read_only_mode;
pub mod guard_share;
pub mod guard_timestamp;
pub mod guard_upload;

pub fn generate_fairing_routes() -> Vec<Route> {
    routes![
        guard_timestamp::renew_timestamp_token,
        guard_hash::renew_hash_token
    ]
}

static VALIDATION: LazyLock<Validation> = LazyLock::new(|| Validation::new(Algorithm::HS256));

static VALIDATION_ALLOW_EXPIRED: LazyLock<Validation> = LazyLock::new(|| {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = false; // Disable expiration validation
    validation
});

#[cfg(test)]
mod tests {
    use jsonwebtoken::{Algorithm, Header};

    use super::{VALIDATION, VALIDATION_ALLOW_EXPIRED};

    // RUSTSEC-2023-0071: rsa 0.9.x has a Marvin Attack timing side-channel.
    // We suppress the advisory in audit.toml because the app exclusively uses
    // HS256 (HMAC). These tests enforce that assumption — if the algorithm
    // is ever changed to an RSA variant, the advisory becomes exploitable.
    #[test]
    fn jwt_default_header_is_hs256() {
        assert_eq!(Header::default().alg, Algorithm::HS256);
    }

    #[test]
    fn jwt_validation_uses_hs256_only() {
        assert_eq!(VALIDATION.algorithms, vec![Algorithm::HS256]);
        assert_eq!(VALIDATION_ALLOW_EXPIRED.algorithms, vec![Algorithm::HS256]);
    }
}
