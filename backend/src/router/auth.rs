#![allow(clippy::module_inception)]

use crate::auth::password::verify_share_password;
use crate::model::album::ResolvedShare;
#[allow(unused_imports)]
use crate::model::album::Share;
use crate::openapi_components::Unauthorized;
use crate::router::{AppResult, GuardError, GuardResult};
use chrono::Utc;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    User { id: String, admin: bool },
    Share(Box<ResolvedShare>),
}

/// Discriminates the three JWT types so a token minted for one decode target
/// cannot be replayed against another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenType {
    /// Identity token: the admin session cookie.
    Admin,
    /// Snapshot token: prefetch's short-lived query token.
    Snapshot,
    /// Asset token: a per-file serving token.
    Asset,
}

/// A claims type that declares which [`TokenType`] it may be decoded from.
pub trait TokenKind {
    const EXPECTED: TokenType;
    fn token_type(&self) -> TokenType;
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Claims {
    pub role: Role,
    pub exp: u64, // seconds since epoch
    /// Type marker; always [`TokenType::Admin`] for identity tokens.
    #[serde(rename = "typ")]
    pub typ: TokenType,
    pub jti: String,
    /// Token version of the user this token was minted for. The guard
    /// rejects a token whose version is not the user's current one: a
    /// password change, a role change or an explicit invalidation bumps the
    /// version and kills every outstanding token.
    ///
    /// Defaults to zero, so a token minted before the version existed still
    /// matches a record that never changed.
    #[serde(default)]
    pub ver: u64,
}

impl Claims {
    /// Mint an identity token for `id` with the given admin flag and token
    /// version.
    #[must_use]
    pub fn new_user(id: String, admin: bool, ver: u64) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let exp = (Utc::now().timestamp_millis() / 1000) as u64 + 14 * 86_400; // 14 days

        Self {
            role: Role::User { id, admin },
            exp,
            typ: TokenType::Admin,
            jti: Uuid::new_v4().to_string(),
            ver,
        }
    }

    pub fn new_share(resolved_share: ResolvedShare) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let exp = (Utc::now().timestamp_millis() / 1000) as u64 + 14 * 86_400; // 14 days

        Self {
            role: Role::Share(Box::new(resolved_share)),
            exp,
            typ: TokenType::Admin,
            jti: Uuid::new_v4().to_string(),
            ver: 0,
        }
    }
    pub fn is_admin(&self) -> bool {
        matches!(self.role, Role::User { admin: true, .. })
    }
    pub fn get_share(&self) -> Option<ResolvedShare> {
        match &self.role {
            Role::Share(share) => Some((**share).clone()),
            Role::User { .. } => None,
        }
    }

    pub fn encode(&self) -> String {
        crate::model::config::with_config(|config| {
            self.encode_with_key(&config.get_jwt_secret_key())
        })
    }

    pub fn encode_with_key(&self, key: &[u8]) -> String {
        encode(&Header::default(), &self, &EncodingKey::from_secret(key))
            .expect("Failed to generate token")
    }
}

impl TokenKind for Claims {
    const EXPECTED: TokenType = TokenType::Admin;
    fn token_type(&self) -> TokenType {
        self.typ
    }
}

// src/router/claims/claims_hash.rs
use arrayvec::ArrayString;

/// JWT claims for image-serving tokens.
///
/// The token carries both identities: `hash` is the asset's content hash used
/// to authorize compressed-thumbnail URLs (validated by [`GuardHash`]), and
/// `asset_id` is the path-primary asset ID used to authorize original-file
/// URLs (validated by [`GuardHashOriginal`]). Album-cover tokens carry the
/// cover image's content hash in `hash` and the album's ID in `asset_id`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ClaimsHash {
    pub allow_original: bool,
    pub hash: ArrayString<64>,
    /// Path-primary asset ID. Tokens and serving resolve by `asset_id`.
    pub asset_id: ArrayString<64>,
    pub timestamp: i64,
    pub exp: u64,
    /// Type marker; always [`TokenType::Asset`].
    #[serde(rename = "typ")]
    pub typ: TokenType,
}

impl ClaimsHash {
    pub fn new(
        hash: ArrayString<64>,
        asset_id: ArrayString<64>,
        timestamp: i64,
        allow_original: bool,
    ) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let exp = (Utc::now().timestamp_millis() / 1000) as u64 + 300;

        Self {
            allow_original,
            hash,
            asset_id,
            timestamp,
            exp,
            typ: TokenType::Asset,
        }
    }

    pub fn encode(&self) -> String {
        let secret_key =
            crate::model::config::with_config(crate::model::config::AppConfig::get_jwt_secret_key);
        encode(
            &Header::default(),
            &self,
            &EncodingKey::from_secret(&secret_key),
        )
        .expect("Failed to generate token")
    }
}

impl TokenKind for ClaimsHash {
    const EXPECTED: TokenType = TokenType::Asset;
    fn token_type(&self) -> TokenType {
        self.typ
    }
}

// src/router/claims/claims_timestamp.rs

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ClaimsTimestamp {
    pub resolved_share_opt: Option<ResolvedShare>,
    pub timestamp: i64,
    pub exp: u64,
    /// Type marker; always [`TokenType::Snapshot`].
    #[serde(rename = "typ")]
    pub typ: TokenType,
}

impl ClaimsTimestamp {
    pub fn new(resolved_share_opt: Option<ResolvedShare>, timestamp: i64) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let exp = (Utc::now().timestamp_millis() / 1000) as u64 + 300;

        Self {
            resolved_share_opt,
            timestamp,
            exp,
            typ: TokenType::Snapshot,
        }
    }

    pub fn encode(&self) -> String {
        let secret_key =
            crate::model::config::with_config(crate::model::config::AppConfig::get_jwt_secret_key);
        encode(
            &Header::default(),
            &self,
            &EncodingKey::from_secret(&secret_key),
        )
        .expect("Failed to generate token")
    }
}

impl TokenKind for ClaimsTimestamp {
    const EXPECTED: TokenType = TokenType::Snapshot;
    fn token_type(&self) -> TokenType {
        self.typ
    }
}

use std::sync::LazyLock;

use jsonwebtoken::{Algorithm, Validation};
use rocket::Route;

pub fn generate_fairing_routes() -> Vec<Route> {
    routes![renew_timestamp_token, renew_hash_token]
}

pub(crate) static VALIDATION: LazyLock<Validation> =
    LazyLock::new(|| Validation::new(Algorithm::HS256));

static VALIDATION_ALLOW_EXPIRED: LazyLock<Validation> = LazyLock::new(|| {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = false; // Disable expiration validation
    validation
});

#[cfg(test)]
mod tests {

    use super::{VALIDATION, VALIDATION_ALLOW_EXPIRED};
    use jsonwebtoken::{Algorithm, Header};

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

    #[test]
    fn claims_new_user_round_trip_preserves_identity() {
        use super::{Claims, decode_typed};
        let _ = &*crate::tests::bootstrap::TEST_ENV;
        let claims = Claims::new_user("alice".to_string(), true, 0);
        let token = claims.encode();
        let decoded: Claims = decode_typed(&token, &VALIDATION).expect("decode round-trip");
        assert!(decoded.is_admin());
        assert_eq!(decoded.typ, super::TokenType::Admin);
        match decoded.role {
            super::Role::User { id, admin } => {
                assert_eq!(id, "alice");
                assert!(admin);
            }
            super::Role::Share(_) => panic!("expected user role"),
        }
        let non_admin = Claims::new_user("bob".to_string(), false, 0);
        assert!(!non_admin.is_admin());
        let token = non_admin.encode();
        let decoded: Claims = decode_typed(&token, &VALIDATION).expect("decode round-trip");
        assert!(!decoded.is_admin());
    }

    #[test]
    fn legacy_admin_token_rejected_by_typed_decode() {
        // A token minted with the old `{"admin": ...}` role shape must fail
        // to decode as the new `Claims` (deny_unknown_fields).
        use super::decode_typed;
        use crate::model::config::APP_CONFIG;
        use jsonwebtoken::{EncodingKey, Header, encode};
        use serde::Serialize;
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct LegacyClaims {
            role: LegacyRole,
            exp: u64,
            #[serde(rename = "typ")]
            typ: super::TokenType,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        enum LegacyRole {
            Admin,
        }
        let _ = &*crate::tests::bootstrap::TEST_ENV;
        let key = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned")
            .get_jwt_secret_key();
        let legacy = LegacyClaims {
            role: LegacyRole::Admin,
            exp: 9_999_999_999,
            typ: super::TokenType::Admin,
        };
        let token = encode(&Header::default(), &legacy, &EncodingKey::from_secret(&key))
            .expect("encode legacy token");
        let result: anyhow::Result<super::Claims> = decode_typed(&token, &VALIDATION);
        assert!(result.is_err(), "legacy Role::Admin token must be rejected");
    }

    fn s3b_client() -> rocket::local::blocking::Client {
        use crate::tests::bootstrap::{TEST_ENV, make_client, reset_backend_state};
        let _ = &*TEST_ENV;
        reset_backend_state();
        make_client()
    }

    fn s3b_lock() -> std::sync::MutexGuard<'static, ()> {
        use crate::tests::bootstrap::TEST_SERIAL_GUARD;
        TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn s3b_set_bootstrap_admin(enabled: bool) {
        use crate::model::config::APP_CONFIG;
        let mut config = APP_CONFIG
            .get()
            .expect("APP_CONFIG set")
            .write()
            .expect("APP_CONFIG lock");
        config.bootstrap_admin = enabled;
    }

    #[test]
    fn jwt_auth_open_window_anonymous_is_401() {
        let _g = s3b_lock();
        let client = s3b_client();
        s3b_set_bootstrap_admin(true);
        // No user record exists: the bootstrap window exposes only
        // server-info and users/bootstrap, so an anonymous admin-guard
        // request is refused with the window-aware enrollment hint.
        let resp = client.get("/get/index/status").dispatch();
        assert_eq!(resp.status(), rocket::http::Status::Unauthorized);
    }

    #[test]
    fn not_enrolled_message_branches_on_window() {
        let _g = s3b_lock();
        let _client = s3b_client();
        s3b_set_bootstrap_admin(true);
        assert!(
            super::not_enrolled_message().contains("POST /post/users/bootstrap"),
            "open window must name the bootstrap endpoint"
        );
        s3b_set_bootstrap_admin(false);
        let closed = super::not_enrolled_message();
        assert!(
            closed.contains("picasu-cmd"),
            "closed wording must keep naming the CLI remedy, got: {closed}"
        );
        assert!(
            closed.contains("--bootstrap-admin"),
            "closed wording must keep naming the flag, got: {closed}"
        );
        s3b_set_bootstrap_admin(true);
    }

    #[test]
    fn jwt_auth_empty_store_flag_off_denies_without_cookie() {
        let _g = s3b_lock();
        let client = s3b_client();
        s3b_set_bootstrap_admin(false);
        let resp = client.get("/get/index/status").dispatch();
        assert_eq!(resp.status(), rocket::http::Status::Unauthorized);
        s3b_set_bootstrap_admin(true);
    }

    #[test]
    fn jwt_auth_valid_admin_user_accepted() {
        let _g = s3b_lock();
        let client = s3b_client();
        crate::auth::users::create_user("s3b-admin-user", true).expect("create user");
        let claims = super::Claims::new_user("s3b-admin-user".to_string(), true, 0);
        let token = claims.encode();
        let resp = client
            .get("/get/index/status")
            .cookie(rocket::http::Cookie::new("jwt", token))
            .dispatch();
        assert_eq!(resp.status(), rocket::http::Status::Ok);
    }

    #[test]
    fn jwt_auth_demoted_user_rejected() {
        let _g = s3b_lock();
        let client = s3b_client();
        crate::auth::users::create_user("s3b-demoted-user", true).expect("create user");
        crate::auth::users::create_user("s3b-other-admin", true).expect("create other admin");
        let claims = super::Claims::new_user("s3b-demoted-user".to_string(), true, 0);
        let token = claims.encode();
        crate::auth::users::set_admin("s3b-demoted-user", false).expect("demote user");
        let resp = client
            .get("/get/index/status")
            .cookie(rocket::http::Cookie::new("jwt", token))
            .dispatch();
        assert_eq!(resp.status(), rocket::http::Status::Unauthorized);
    }

    #[test]
    fn jwt_auth_unknown_and_non_admin_users_rejected() {
        let _g = s3b_lock();
        let client = s3b_client();
        crate::auth::users::create_user("s3b-other-user", false).expect("create user");
        // Unknown user: store is non-empty but the id has no record.
        let ghost = super::Claims::new_user("s3b-ghost-user".to_string(), true, 0).encode();
        let resp = client
            .get("/get/index/status")
            .cookie(rocket::http::Cookie::new("jwt", ghost))
            .dispatch();
        assert_eq!(resp.status(), rocket::http::Status::Unauthorized);
        // Non-admin user with a non-admin token.
        let plain = super::Claims::new_user("s3b-other-user".to_string(), false, 0).encode();
        let resp = client
            .get("/get/index/status")
            .cookie(rocket::http::Cookie::new("jwt", plain))
            .dispatch();
        assert_eq!(resp.status(), rocket::http::Status::Unauthorized);
    }

    // ——— R6 step 2: jti enforcement (RED before feature) ———

    #[test]
    fn legacy_token_without_jti_rejected() {
        use crate::model::config::APP_CONFIG;
        use jsonwebtoken::{EncodingKey, Header, encode};
        use serde::Serialize;
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct LegacyNoJtiClaims {
            role: super::Role,
            exp: u64,
            #[serde(rename = "typ")]
            typ: super::TokenType,
        }
        let _g = s3b_lock();
        let _client = s3b_client();
        let _ = &*crate::tests::bootstrap::TEST_ENV;
        let key = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned")
            .get_jwt_secret_key();
        let legacy = LegacyNoJtiClaims {
            role: super::Role::User {
                id: "legacy-user".to_string(),
                admin: true,
            },
            exp: 9_999_999_999,
            typ: super::TokenType::Admin,
        };
        let token = encode(&Header::default(), &legacy, &EncodingKey::from_secret(&key))
            .expect("encode legacy token without jti");
        let result: anyhow::Result<super::Claims> = super::decode_typed(&token, &super::VALIDATION);
        assert!(
            result.is_err(),
            "token without jti must be rejected after hard cut"
        );
    }

    #[test]
    fn token_with_a_stale_version_rejected() {
        let _g = s3b_lock();
        let client = s3b_client();
        // The token version is the user's current one plus one: a password
        // change, a role change or an explicit invalidation bumped it, so
        // the token is stale and must be refused.
        crate::auth::users::create_user("s3b-jti-user", true).expect("create user");
        let token = super::Claims::new_user("s3b-jti-user".to_string(), true, 1).encode();
        let resp = client
            .get("/get/index/status")
            .cookie(rocket::http::Cookie::new("jwt", token))
            .dispatch();
        assert_eq!(
            resp.status(),
            rocket::http::Status::Unauthorized,
            "a token with a stale version must be 401"
        );
    }

    #[test]
    fn token_with_the_current_version_accepted() {
        let _g = s3b_lock();
        let client = s3b_client();
        // The server keeps no session list: a token is accepted because it
        // carries the current version, not because the server remembers it.
        crate::auth::users::create_user("s3b-ver-user", true).expect("create user");
        let ver = crate::auth::users::user_record("s3b-ver-user")
            .expect("get")
            .expect("the user must exist")
            .ver;
        let token = super::Claims::new_user("s3b-ver-user".to_string(), true, ver).encode();
        let resp = client
            .get("/get/index/status")
            .cookie(rocket::http::Cookie::new("jwt", token))
            .dispatch();
        assert_eq!(resp.status(), rocket::http::Status::Ok);
    }
}

// src/router/fairing/auth_utils.rs
use crate::error::{AppError, ErrorKind};
use crate::model::metadata_record::MetadataRecord;
use crate::storage::db::METADATA_TABLE;
use crate::storage::db::TREE;

use anyhow::{Error, Result, anyhow};
use jsonwebtoken::{DecodingKey, decode};
use log::info;
use redb::ReadableDatabase;
use rocket::Request;
use serde::de::DeserializeOwned;

// Error types for share validation are now handled by AppError

/// Extract and validate Authorization header Bearer token.
///
/// Header-only: the query form (`?token=`) leaked credentials into proxy
/// logs, browser history and `Referer` headers. URL-passable credentials
/// keep their own shapes instead — share links carry a path slug and the
/// `x-album-id`/`x-share-id` (or `albumId`/`shareId`) pair, invite links
/// carry `?invite=` in the frontend route and redeem via POST body.
pub fn extract_bearer_token<'a>(req: &'a Request<'_>) -> Result<&'a str> {
    if let Some(auth_header) = req.headers().get_one("Authorization") {
        match auth_header.strip_prefix("Bearer ") {
            Some(token) => return Ok(token),
            None => {
                return Err(anyhow!(
                    "Authorization header format is invalid, expected 'Bearer <token>'"
                ));
            }
        }
    }

    Err(anyhow!("Request is missing the Authorization header"))
}

/// Decode JWT token with given claims type and validation
pub fn my_decode_token<T: DeserializeOwned>(token: &str, validation: &Validation) -> Result<T> {
    let secret_key =
        crate::model::config::with_config(crate::model::config::AppConfig::get_jwt_secret_key);

    match decode::<T>(token, &DecodingKey::from_secret(&secret_key), validation) {
        Ok(token_data) => Ok(token_data.claims),
        Err(err) => {
            info!("Token decode failed: {err:?}");
            Err(Error::from(err).context("Failed to decode JWT token"))
        }
    }
}

/// Decode a JWT and enforce that its `typ` claim matches the target claims
/// type, so a token minted for one decode target cannot be replayed against
/// another. `deny_unknown_fields` on the claims structs already rejects the
/// cross-type shapes; this is the explicit type check on top.
pub fn decode_typed<T>(token: &str, validation: &Validation) -> Result<T>
where
    T: DeserializeOwned + TokenKind,
{
    let claims: T = my_decode_token(token, validation)?;
    if claims.token_type() != T::EXPECTED {
        return Err(anyhow!(
            "Token type mismatch: expected {:?}, found {:?}",
            T::EXPECTED,
            claims.token_type()
        ));
    }
    Ok(claims)
}

/// Whether the open bootstrap window is available: the `--bootstrap-admin`
/// flag is set.
///
/// Callers combine this with their own store-emptiness check; the flag alone
/// never opens a non-empty store.
pub(crate) fn bootstrap_open() -> bool {
    crate::model::config::with_config(|c| c.bootstrap_admin)
}

/// Refusal message for an empty user store, branched once on the window
/// state: while the bootstrap window is open the message names the only
/// enrollment act (`POST /post/users/bootstrap`); otherwise it keeps the
/// closed wording (flag/CLI remedies).
pub(crate) fn not_enrolled_message() -> &'static str {
    if bootstrap_open() {
        "No administrator enrolled: enroll the first administrator via POST /post/users/bootstrap"
    } else {
        "No administrator enrolled: start the server with --bootstrap-admin (or PICASU_BOOTSTRAP_ADMIN=true) to enroll the first admin, or provision a user with the picasu-cmd CLI"
    }
}

/// [`not_enrolled_message`] as an `anyhow::Error` for the `try_*` guards.
pub(crate) fn not_enrolled_error() -> anyhow::Error {
    anyhow::anyhow!(not_enrolled_message())
}

/// Try to authenticate via JWT cookie and check if user is admin.
///
/// No session exists on an empty store: the bootstrap window enrolls only
/// via `POST /post/users/bootstrap`, so every guarded route refuses with
/// [`not_enrolled_error`] (the message alone branches on the window state).
/// On a non-empty store the token must carry [`Role::User`] with
/// `admin: true`, and the identity record is re-read from the user store on
/// every call, so a demotion, a removal or an invalidated token takes
/// effect before the token expires.
pub fn try_jwt_cookie_auth(req: &Request<'_>, validation: &Validation) -> Result<Claims> {
    let store_non_empty =
        !crate::auth::user_store::is_empty(&crate::auth::users::user_store_path());
    if !store_non_empty {
        return Err(not_enrolled_error());
    }

    if let Some(jwt_cookie) = req.cookies().get("jwt") {
        let token = jwt_cookie.value();
        let claims = decode_typed::<Claims>(token, validation)?;
        if !claims.is_admin() {
            return Err(anyhow!("User is not an admin"));
        }
        let Role::User { id, .. } = &claims.role else {
            return Err(anyhow!("User is not an admin"));
        };
        let passwd_admin = crate::auth::users::get_admin(id)
            .ok()
            .flatten()
            .unwrap_or(false);
        match crate::auth::users::user_record(id)? {
            Some(record) if passwd_admin => {
                if record.ver != claims.ver {
                    return Err(anyhow!("User is not an admin"));
                }
                if crate::auth::revocations::is_revoked(&crate::auth::revocations::session_key(
                    &claims.jti,
                )) {
                    return Err(anyhow!("User is not an admin"));
                }
                return Ok(claims);
            }
            _ => return Err(anyhow!("User is not an admin")),
        }
    }
    Err(anyhow!("JWT not found in cookies"))
}

/// Try to authenticate via JWT cookie as any live user, admin or not.
///
/// Same contract as [`try_jwt_cookie_auth`] minus the admin requirement: no
/// session exists on an empty store, the token must carry [`Role::User`]
/// (share-role cookies are rejected), and the identity record is re-read from
/// the user store on every call — so a stale token version, a removal or a
/// revoked `jti` takes effect before the token expires, and the admin flag is
/// taken from the record rather than the token.
pub fn try_jwt_cookie_user(req: &Request<'_>, validation: &Validation) -> Result<Claims> {
    let store_non_empty =
        !crate::auth::user_store::is_empty(&crate::auth::users::user_store_path());
    if !store_non_empty {
        return Err(not_enrolled_error());
    }

    let Some(jwt_cookie) = req.cookies().get("jwt") else {
        return Err(anyhow!("JWT not found in cookies"));
    };
    let mut claims = decode_typed::<Claims>(jwt_cookie.value(), validation)?;
    let Role::User { id, .. } = &claims.role else {
        return Err(anyhow!("Share token not accepted"));
    };
    let id = id.clone();
    match crate::auth::users::user_record(&id)? {
        Some(record) if record.ver == claims.ver => {
            if crate::auth::revocations::is_revoked(&crate::auth::revocations::session_key(
                &claims.jti,
            )) {
                return Err(anyhow!("Session revoked"));
            }
            if let Role::User { admin, .. } = &mut claims.role {
                *admin = record.admin;
            }
            Ok(claims)
        }
        _ => Err(anyhow!("User record missing or stale")),
    }
}

/// Extract the serving ID from the request URL path (last segment before the
/// extension).
///
/// What the segment means depends on the guard that calls this: for
/// compressed-thumbnail serving it is the asset's content hash (compared
/// against the `hash` claim), and for original-file serving it is the
/// path-primary `asset_id` (compared against the `asset_id` claim).
pub fn extract_serving_id_from_path(req: &Request<'_>) -> Result<String> {
    let id_opt = req
        .uri()
        .path()
        .segments()
        .last()
        .and_then(|segment_with_ext| segment_with_ext.rsplit_once('.'))
        .map(|(id, _ext)| id.to_string());

    match id_opt {
        Some(id) => Ok(id),
        // Message kept verbatim for wire compatibility; the segment itself is
        // a content hash (compressed) or an asset ID (original).
        None => Err(anyhow!("No valid 'hash' parameter found in the uri")),
    }
}

/// Validate share access: check expiration and password
fn validate_share_access(share: &Share, req: &Request<'_>) -> Result<(), AppError> {
    // 1. Check expiration
    if share.exp > 0 {
        let now = Utc::now().timestamp_millis() / 1000;

        if now > share.exp {
            return Err(AppError::new(
                ErrorKind::PermissionDenied,
                "Share link expired",
            ));
        }
    }

    // 2. Check password
    if let Some(ref pwd) = share.password {
        // Check Header: x-share-password
        if let Some(header_pwd) = req.headers().get_one("x-share-password")
            && verify_share_password(header_pwd, pwd)
        {
            return Ok(());
        }

        return Err(AppError::new(
            ErrorKind::Auth,
            "Share password required or incorrect",
        ));
    }

    Ok(())
}

/// Re-read a share from the DB and confirm it is still live: it exists and has
/// not expired. Used at token renewal so a share that has since been disabled
/// or expired stops yielding refreshed capabilities.
///
/// The password is not re-checked here: the presenter's own `GuardShare`
/// already validated it for the share being renewed, and an admin presenter is
/// trusted. This is the DB-state half of re-validation.
fn revalidate_share_record(album_id: &str, share_id: &str) -> Result<(), AppError> {
    let read_txn = TREE.in_disk.begin_read().map_err(|e| {
        AppError::from_err(ErrorKind::Database, e.into())
            .context("Failed to begin read transaction")
    })?;
    let table = read_txn.open_table(METADATA_TABLE).map_err(|e| {
        AppError::from_err(ErrorKind::Database, e.into()).context("Failed to open data table")
    })?;
    let data = table
        .get(album_id)
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
        .ok_or_else(|| AppError::new(ErrorKind::Auth, "Share no longer exists"))?;
    let MetadataRecord::Album(mut album) = data.value() else {
        return Err(AppError::new(ErrorKind::Auth, "Share no longer exists"));
    };
    let share = album
        .share_list
        .remove(share_id)
        .ok_or_else(|| AppError::new(ErrorKind::Auth, "Share no longer exists"))?;
    if share.exp > 0 && Utc::now().timestamp_millis() / 1000 > share.exp {
        return Err(AppError::new(ErrorKind::Auth, "Share link expired"));
    }
    Ok(())
}

fn resolve_share_internal(
    album_id: &str,
    share_id: &str,
    req: &Request<'_>,
) -> Result<Option<Claims>, AppError> {
    let read_txn = TREE.in_disk.begin_read().map_err(|e| {
        AppError::from_err(ErrorKind::Database, e.into())
            .context("Failed to begin read transaction")
    })?;

    let table = read_txn.open_table(METADATA_TABLE).map_err(|e| {
        AppError::from_err(ErrorKind::Database, e.into()).context("Failed to open data table")
    })?;

    let data_guard = table
        .get(album_id)
        .map_err(|e| {
            AppError::from_err(ErrorKind::Database, e.into())
                .context("Failed to get data from table")
        })?
        .ok_or_else(|| {
            AppError::new(
                ErrorKind::NotFound,
                format!("Album not found for id '{album_id}'"),
            )
        })?;

    // Share lookup reads the metadata-only payload; the album's identity id
    // is the table key itself (`asset_id`).
    let MetadataRecord::Album(mut album) = data_guard.value() else {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!("Data with id '{album_id}' is not an album"),
        ));
    };

    let mut share = album.share_list.remove(share_id).ok_or_else(|| {
        AppError::new(
            ErrorKind::NotFound,
            format!("Share '{share_id}' not found in album '{album_id}'"),
        )
    })?;

    // Keep the credential the caller presented so the resolved share can
    // echo it for the capability chain; at rest the stored value stays
    // hashed.
    let presented_password = req.headers().get_one("x-share-password").map(str::to_owned);

    // Validate share access (password and expiration)
    validate_share_access(&share, req)?;

    // Echo the presented password (not the stored hash) in the resolved
    // share, so later media requests resend a usable credential.
    if share.password.is_some()
        && let Some(pw) = presented_password
    {
        share.password = Some(pw);
    }

    let resolved_share = ResolvedShare::new(
        ArrayString::<64>::from(album_id)
            .map_err(|_| AppError::new(ErrorKind::Internal, "Failed to parse album_id"))?,
        album.title,
        share,
    );
    let claims = Claims::new_share(resolved_share);
    Ok(Some(claims))
}

/// Try to resolve album and share from headers
pub fn try_resolve_share_from_headers(req: &Request<'_>) -> Result<Option<Claims>, AppError> {
    let album_id = req.headers().get_one("x-album-id");
    let share_id = req.headers().get_one("x-share-id");

    match (album_id, share_id) {
        (None, None) => Ok(None),

        (Some(_), None) | (None, Some(_)) => Err(AppError::new(
            ErrorKind::InvalidInput,
            "Both x-album-id and x-share-id must be provided together",
        )),

        (Some(album_id), Some(share_id)) => resolve_share_internal(album_id, share_id, req),
    }
}

/// Try to resolve album and share from query parameters
pub fn try_resolve_share_from_query(req: &Request<'_>) -> Result<Option<Claims>, AppError> {
    let album_id = req.query_value::<&str>("albumId").and_then(Result::ok);
    let share_id = req.query_value::<&str>("shareId").and_then(Result::ok);

    match (album_id, share_id) {
        (None, None) => Ok(None),

        (Some(_), None) | (None, Some(_)) => Err(AppError::new(
            ErrorKind::InvalidInput,
            "Both albumId and shareId must be provided together",
        )),

        (Some(album_id), Some(share_id)) => resolve_share_internal(album_id, share_id, req),
    }
}

/// Try to authorize upload via share headers with upload permission
pub fn try_authorize_upload_via_share(req: &Request<'_>) -> bool {
    if let Some(album_id) = req.headers().get_one("x-album-id")
        && let Some(share_id) = req.headers().get_one("x-share-id")
        && let Ok(read_txn) = TREE.in_disk.begin_read()
        && let Ok(table) = read_txn.open_table(METADATA_TABLE)
        && let Ok(Some(data_guard)) = table.get(album_id)
        && let MetadataRecord::Album(mut album) = data_guard.value()
        && let Some(share) = album.share_list.remove(share_id)
        && share.show_upload
        && validate_share_access(&share, req).is_ok()
        && let Some(Ok(album_id_parsed)) = req.query_value::<&str>("presigned_album_id_opt")
    {
        // The payload is keyed by the album's `asset_id`.
        return album_id == album_id_parsed;
    }

    false
}

// src/router/fairing/guard_auth.rs
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};

pub struct GuardAuth {
    pub claims: Claims,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardAuth {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match try_jwt_cookie_auth(req, &VALIDATION) {
            Ok(claims) => Outcome::Success(GuardAuth { claims }),
            Err(err) => Outcome::Error((
                Status::Unauthorized,
                AppError::from_err(ErrorKind::Auth, err).context("Authentication error"),
            )),
        }
    }
}

/// Request guard for any authenticated user (admin or not). Carries the
/// caller's claims with the admin flag re-read from the DB, so demotion
/// takes effect before token expiry.
///
/// Unlike [`GuardAuth`] (admin-only), this admits non-admin users, for the
/// self-service password routes. Share-role tokens are rejected.
pub struct GuardUser {
    pub claims: Claims,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardUser {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // Tier taxonomy: 401 means the presented credential (cookie) was not
        // accepted — session-tier only, so the SPA may clear its display
        // profile. Privilege/administration refusals (no-admin-enrolled) are
        // not session failures: they return 403 so a client never loses its
        // display state over an install-state condition.
        let denied = |msg: &str| {
            Outcome::Error((
                Status::Unauthorized,
                AppError::new(ErrorKind::Auth, msg).context("Authentication error"),
            ))
        };
        let forbidden = |msg: &str| {
            Outcome::Error((
                Status::Forbidden,
                AppError::new(ErrorKind::PermissionDenied, msg).context("Authorization error"),
            ))
        };
        let non_empty = match crate::auth::users::user_count() {
            Ok(count) => count > 0,
            Err(err) => {
                return Outcome::Error((
                    Status::InternalServerError,
                    AppError::from_err(ErrorKind::Database, err),
                ));
            }
        };
        if !non_empty {
            // No session exists pre-enrollment: refuse with the
            // privilege-tier status (403, never a session failure) and the
            // window-state message.
            return forbidden(not_enrolled_message());
        }
        let Some(jwt_cookie) = req.cookies().get("jwt") else {
            return denied("JWT not found in cookies");
        };
        let claims: Claims = match decode_typed(jwt_cookie.value(), &VALIDATION) {
            Ok(claims) => claims,
            Err(_) => return denied("Invalid token"),
        };
        let id = match &claims.role {
            Role::User { id, .. } => id.clone(),
            Role::Share(_) => return denied("Share token not accepted"),
        };
        // Identity lives in the user store; the token carries the version it
        // was minted with. A token whose version is stale was invalidated by
        // a password change, a role change or an explicit invalidation.
        let record = match crate::auth::users::user_record(&id) {
            Ok(Some(record)) => record,
            Ok(None) => return denied("Unknown user"),
            Err(err) => {
                return Outcome::Error((
                    Status::InternalServerError,
                    AppError::from_err(ErrorKind::Database, err),
                ));
            }
        };
        if record.ver != claims.ver {
            return denied("Invalid token version");
        }
        // A revoked session (this device logged out) is dead even though its
        // token still verifies.
        if crate::auth::revocations::is_revoked(&crate::auth::revocations::session_key(&claims.jti))
        {
            return denied("Session revoked");
        }
        // The admin flag comes from the record, not the token: a demotion
        // takes effect at once. A promotion bumps the version, so the old
        // token is already dead and the new role appears at the next
        // sign-in.
        let mut claims = claims;
        if let Role::User { admin, .. } = &mut claims.role {
            *admin = record.admin;
        }
        Outcome::Success(GuardUser { claims })
    }
}

// src/router/fairing/guard_reauth.rs

/// Request guard for re-authentication: a valid admin/user session (resolved
/// from the `jwt` cookie) plus the account password presented in the
/// `X-Picasu-Password` request header, verified against the caller's stored
/// hash via `UserStore::verify`. Used by the read-only lift and the
/// self-service password change, so a captured token alone cannot perform
/// either.
///
/// Session failures surface as 401; a missing or wrong password as 403.
pub struct GuardReauth {
    #[allow(dead_code)]
    pub claims: Claims,
}

#[rocket::async_trait]
#[allow(clippy::too_many_lines)]
impl<'r> FromRequest<'r> for GuardReauth {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let Ok(claims) = try_jwt_cookie_user(req, &VALIDATION) else {
            return Outcome::Error((
                Status::Unauthorized,
                AppError::new(ErrorKind::Auth, "Authentication error"),
            ));
        };
        let Role::User { id, .. } = &claims.role else {
            return Outcome::Error((
                Status::Unauthorized,
                AppError::new(ErrorKind::Auth, "Authentication error"),
            ));
        };
        let id = id.clone();

        let ip = match crate::router::post::users_authenticate::ThrottleClientIp::from_request(req)
            .await
        {
            Outcome::Success(t) => t.0,
            _ => String::new(),
        };
        let rate_limits_disabled = crate::router::post::users_authenticate::rate_limits_disabled();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .ok();
        let throttle = if rate_limits_disabled {
            None
        } else {
            now.map(
                |now| crate::router::put::users_password::PasswordThrottleCtx {
                    ip,
                    jti: claims.jti.clone(),
                    exp: claims.exp,
                    now,
                },
            )
        };

        let Some(presented) = req.headers().get_one("x-picasu-password") else {
            return Outcome::Error((
                Status::Forbidden,
                AppError::new(
                    ErrorKind::PermissionDenied,
                    "Re-authentication password required",
                ),
            ));
        };
        let presented = presented.to_string();

        let path = crate::auth::users::user_store_path();
        let Ok(store) = crate::auth::user_store::UserStore::load(&path) else {
            return Outcome::Error((
                Status::InternalServerError,
                AppError::new(ErrorKind::IO, "User store unavailable"),
            ));
        };

        if let Some(ctx) = &throttle {
            let ban = crate::router::put::users_password::with_password_throttle(|t| {
                t.reserve(&ctx.ip, &ctx.jti, ctx.now)
            });
            if ban > 0 {
                crate::router::put::users_password::with_password_throttle(|t| t.sweep(ctx.now));
                let _ =
                    crate::router::put::users_password::revoke_attempt_session(&ctx.jti, ctx.exp);
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::new(
                        ErrorKind::Auth,
                        "too many incorrect password attempts; session revoked",
                    ),
                ));
            }
        }

        if !store.verify(&id, &presented) {
            if let Some(ctx) = &throttle {
                let ban = crate::router::put::users_password::with_password_throttle(|t| {
                    t.release(&ctx.ip, &ctx.jti);
                    t.record_failure(&ctx.ip, &ctx.jti, ctx.now)
                });
                if ban > 0 {
                    let _ = crate::router::put::users_password::revoke_attempt_session(
                        &ctx.jti, ctx.exp,
                    );
                    return Outcome::Error((
                        Status::Unauthorized,
                        AppError::new(
                            ErrorKind::Auth,
                            "too many incorrect password attempts; session revoked",
                        ),
                    ));
                }
            }
            let detail = match &throttle {
                None => "Incorrect current password".to_string(),
                Some(ctx) => {
                    match crate::router::put::users_password::with_password_throttle(|t| {
                        t.remaining(&ctx.ip, &ctx.jti, ctx.now)
                    }) {
                        0 => "Incorrect current password (no attempts remaining)".to_string(),
                        1 => "Incorrect current password (1 attempt remaining)".to_string(),
                        n => format!("Incorrect current password ({n} attempts remaining)"),
                    }
                }
            };
            return Outcome::Error((
                Status::Forbidden,
                AppError::new(ErrorKind::PermissionDenied, detail),
            ));
        }

        if let Some(ctx) = &throttle {
            crate::router::put::users_password::with_password_throttle(|t| {
                t.record_success(&ctx.ip, &ctx.jti, ctx.now);
            });
        }

        Outcome::Success(GuardReauth { claims })
    }
}

// src/router/fairing/guard_hash.rs
use log::warn;
use rocket::serde::json::Json;

use crate::error::ResultExt;

/// Request guard for compressed image serving: the serving ID extracted from
/// the URL path must equal the token's content `hash` claim.
pub struct GuardHash;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardHash {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let token = match extract_bearer_token(req) {
            Ok(token) => token,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err)
                        .context("Bearer token extraction failed"),
                ));
            }
        };

        let claims: ClaimsHash = match decode_typed(token, &VALIDATION) {
            Ok(claims) => claims,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err).context("JWT decoding failed"),
                ));
            }
        };

        let data_hash = match extract_serving_id_from_path(req) {
            Ok(hash) => hash,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err).context("Hash extraction failed"),
                ));
            }
        };

        // Compare hash in the token with the hash in the request path
        if data_hash != *claims.hash {
            warn!(
                "Hash does not match. Received: {}, Expected: {}.",
                data_hash, claims.hash
            );
            return Outcome::Error((
                Status::Unauthorized,
                AppError::new(ErrorKind::Auth, "Hash does not match"),
            ));
        }
        Outcome::Success(GuardHash)
    }
}

/// Request guard for original-file serving: the asset ID extracted from the
/// URL path must equal the token's `asset_id` claim (asset ID is
/// authoritative — no hash fallback).
pub struct GuardHashOriginal;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardHashOriginal {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let token = match extract_bearer_token(req) {
            Ok(token) => token,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err)
                        .context("Bearer token extraction failed"),
                ));
            }
        };

        let claims: ClaimsHash = match decode_typed(token, &VALIDATION) {
            Ok(claims) => claims,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err).context("JWT decoding failed"),
                ));
            }
        };

        if !claims.allow_original {
            warn!("Original hash access is not allowed.");
            return Outcome::Error((
                Status::Unauthorized,
                AppError::new(ErrorKind::Auth, "Original access is not allowed"),
            ));
        }

        // Extract the asset_id from the URL path.
        let url_id = match extract_serving_id_from_path(req) {
            Ok(id) => id,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err)
                        .context("Asset ID extraction from URL failed"),
                ));
            }
        };

        // Validate against the token's asset_id.
        // Asset ID is authoritative — no hash fallback.
        if url_id != *claims.asset_id {
            warn!(
                "Asset ID does not match. URL: {url_id}, Token: {}.",
                claims.asset_id
            );
            return Outcome::Error((
                Status::Unauthorized,
                AppError::new(ErrorKind::Auth, "Asset ID does not match"),
            ));
        }
        Outcome::Success(GuardHashOriginal)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct RenewHashToken {
    pub expired_hash_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct RenewHashTokenReturn {
    pub token: String,
}

/// Exchange an expired image-serving token for a freshly issued one.
///
/// The submitted token is decoded with expiration checking disabled, so an
/// expired but correctly signed token is accepted. The returned token keeps
/// the original `hash`, `assetId` and `allowOriginal` claims and expires 300
/// seconds after it is issued.
///
/// Corner cases: The request must also carry a valid, unexpired prefetch
/// timestamp bearer token, and the `timestamp` claim of the submitted token
/// must match it — renewing only extends the lifetime of the same snapshot.
///
/// Errors: 400 unusable request body — 401 unverifiable signature, mismatched
/// `timestamp`, or a missing or invalid timestamp bearer token — 500 internal
/// failure.
#[utoipa::path(
        tag = "auth",
        request_body = RenewHashToken,
        security(("bearer_token" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["TimestampGuardModified"],
            "tokenTyp": ["snapshot"],
            "admin": "n-a",
            "shareScope": "n-a",
            "readonly": false,
            "reauth": false,
            "public": false,
            "effects": [],
            "invariants": ["AUTH-10"]
        }))),
        responses(
            (status = 200, description = "Hash token renewed", body = RenewHashTokenReturn),
            (status = 400, description = "Invalid input"),
            (status = 404, description = "Not found"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/renew-hash-token", format = "json", data = "<token_request>")]
pub async fn renew_hash_token(
    auth: TimestampGuardModified,
    token_request: Json<RenewHashToken>,
) -> AppResult<Json<RenewHashTokenReturn>> {
    tokio::task::spawn_blocking(move || {
        let expired_hash_token = token_request.into_inner().expired_hash_token;
        let claims: ClaimsHash =
            match decode_typed::<ClaimsHash>(&expired_hash_token, &VALIDATION_ALLOW_EXPIRED) {
                Ok(claims) => claims,
                Err(err) => {
                    warn!("Token renewal failed: unable to decode token. Error: {err:#?}");
                    return Err(AppError::new(
                        ErrorKind::Auth,
                        "Unauthorized: Invalid token",
                    ));
                }
            };

        if claims.timestamp != auth.claims.timestamp {
            warn!(
                "Timestamp does not match. Received: {}, Expected: {}",
                claims.timestamp, auth.claims.timestamp
            );
            return Err(AppError::new(
                ErrorKind::Auth,
                "Unauthorized: Timestamp mismatch",
            ));
        }

        // Re-validate the presenter's share (if any) from the DB, so a share
        // that has since been disabled or expired stops renewing capabilities.
        if let Some(resolved) = &auth.claims.resolved_share_opt {
            revalidate_share_record(&resolved.album_id, &resolved.share.url)?;
        }

        let new_hash_claims = ClaimsHash::new(
            claims.hash,
            claims.asset_id,
            claims.timestamp,
            claims.allow_original,
        );
        let new_hash_token = new_hash_claims.encode();

        Ok(Json(RenewHashTokenReturn {
            token: new_hash_token,
        }))
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?
}

/// Presenter guard for hash-token renewal: a valid `ClaimsTimestamp` bearer.
/// Carries the full claims so the handler can re-validate the presenter's
/// share before re-issuing an asset token.
pub struct TimestampGuardModified {
    pub claims: ClaimsTimestamp,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for TimestampGuardModified {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let token = match extract_bearer_token(req) {
            Ok(token) => token,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err)
                        .context("Bearer token extraction failed"),
                ));
            }
        };

        let claims: ClaimsTimestamp = match decode_typed(token, &VALIDATION) {
            Ok(claims) => claims,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err).context("JWT decoding failed"),
                ));
            }
        };
        Outcome::Success(TimestampGuardModified { claims })
    }
}

// src/router/fairing/guard_read_only_mode.rs

pub struct GuardReadOnlyMode;
#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardReadOnlyMode {
    type Error = GuardError;
    async fn from_request(_req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        if crate::model::config::with_config(|c| c.read_only_mode) {
            return Outcome::Error((
                Status::MethodNotAllowed,
                AppError::new(ErrorKind::ReadOnlyMode, "Read-only mode is enabled"),
            ));
        }

        Outcome::Success(GuardReadOnlyMode)
    }
}

pub struct GuardShare {
    pub claims: Claims,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardShare {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // headers
        match try_resolve_share_from_headers(req) {
            Ok(Some(claims)) => return Outcome::Success(GuardShare { claims }),
            Ok(None) => {} // No share headers, continue
            Err(err) => {
                let status = err.http_status();
                return Outcome::Error((status, err));
            }
        }

        // query
        match try_resolve_share_from_query(req) {
            Ok(Some(claims)) => return Outcome::Success(GuardShare { claims }),
            Ok(None) => {}
            Err(err) => {
                let status = err.http_status();
                return Outcome::Error((status, err));
            }
        }

        // Fall back to JWT cookie authentication (any live user, admin or not)
        match try_jwt_cookie_user(req, &VALIDATION) {
            Ok(claims) => return Outcome::Success(GuardShare { claims }),
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err).context("Authentication error"),
                ));
            }
        }
    }
}

pub struct GuardTimestamp {
    pub claims: ClaimsTimestamp,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardTimestamp {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let token = match extract_bearer_token(req) {
            Ok(token) => token,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err),
                ));
            }
        };

        let claims: ClaimsTimestamp = match decode_typed(token, &VALIDATION) {
            Ok(claims) => claims,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err),
                ));
            }
        };

        let maybe_timestamp = req.uri().query().and_then(|query| {
            query
                .segments()
                .find(|(key, _)| *key == "timestamp")
                .and_then(|(_, value)| value.parse::<i64>().ok())
        });

        let Some(query_timestamp) = maybe_timestamp else {
            return Outcome::Error((
                Status::Unauthorized,
                AppError::new(
                    ErrorKind::Auth,
                    "No valid 'timestamp' parameter found in the query",
                ),
            ));
        };

        if query_timestamp != claims.timestamp {
            warn!(
                "Timestamp does not match; received: {}; expected: {}",
                query_timestamp, claims.timestamp
            );
            return Outcome::Error((
                Status::Unauthorized,
                AppError::new(ErrorKind::Auth, "Timestamp mismatch"),
            ));
        }

        Outcome::Success(GuardTimestamp { claims })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct RenewTimestampToken {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct RenewTimestampTokenReturn {
    pub token: String,
}

/// Exchange an expired prefetch timestamp token for a freshly issued one.
///
/// Accepts share credentials (`x-album-id` + `x-share-id` headers or
/// `albumId` + `shareId` query parameters) or an admin JWT cookie. The
/// submitted token is decoded with expiration checking disabled and reissued
/// with its original snapshot `timestamp` and resolved share intact, expiring
/// 300 seconds later.
///
/// A share presenter may only renew its own share's snapshot; the embedded
/// share is also re-resolved from the DB before re-issue, so a share that has
/// since been disabled or expired stops yielding refreshed tokens. An admin
/// presenter may renew any snapshot, but the embedded share is still
/// re-validated.
///
/// Corner cases: Re-renewing keeps addressing the same snapshot, so only the
/// expiry changes and the underlying data is not re-read.
///
/// Errors: 400 only one of the share credential header or query pair given —
/// 401 missing or invalid credentials, a token whose share differs from the
/// presenter's, an expired or removed embedded share, or an unverifiable token
/// signature — 500 internal failure.
#[utoipa::path(
        tag = "auth",
        request_body = RenewTimestampToken,
        security(("album_id" = [], "share_id" = []), ("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardShare"],
            "tokenTyp": ["admin", "share"],
            "admin": "either",
            "shareScope": "own-album",
            "readonly": false,
            "reauth": false,
            "public": false,
            "effects": [],
            "invariants": ["AUTH-10"]
        }))),
        responses(
            (status = 200, description = "Timestamp token renewed", body = RenewTimestampTokenReturn),
            (status = 400, description = "Invalid input"),
            (status = 404, description = "Not found"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post(
    "/post/renew-timestamp-token",
    format = "json",
    data = "<token_request>"
)]
pub async fn renew_timestamp_token(
    auth: GuardResult<GuardShare>,
    token_request: Json<RenewTimestampToken>,
) -> AppResult<Json<RenewTimestampTokenReturn>> {
    let presenter = auth?;
    let presenter_share = presenter.claims.get_share();
    tokio::task::spawn_blocking(move || {
        let token = token_request.into_inner().token;
        let claims: ClaimsTimestamp =
            match decode_typed::<ClaimsTimestamp>(&token, &VALIDATION_ALLOW_EXPIRED) {
                Ok(claims) => claims,
                Err(err) => {
                    warn!("Token renewal failed: unable to decode token, error: {err:#?}");
                    return Err(AppError::new(
                        ErrorKind::Auth,
                        "Unauthorized: Invalid token",
                    ));
                }
            };

        // Bind the submitted token's share to the presenter's share: a share
        // presenter may only renew its own snapshot. Admin presenters may
        // renew any, but the embedded share is still re-validated below.
        if let Some(presenter) = &presenter_share {
            let same = claims.resolved_share_opt.as_ref().is_some_and(|s| {
                s.album_id == presenter.album_id && s.share.url == presenter.share.url
            });
            if !same {
                warn!("Renewal share mismatch: presenter and token share differ");
                return Err(AppError::new(
                    ErrorKind::Auth,
                    "Unauthorized: Share mismatch",
                ));
            }
        }

        // Re-validate the embedded share from the DB before re-issuing, so a
        // share that has since been disabled or expired stops renewing.
        if let Some(resolved) = &claims.resolved_share_opt {
            revalidate_share_record(&resolved.album_id, &resolved.share.url)?;
        }

        let new_claims = ClaimsTimestamp::new(claims.resolved_share_opt, claims.timestamp);
        let new_token = new_claims.encode();

        Ok(Json(RenewTimestampTokenReturn { token: new_token }))
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?
}

pub struct GuardUpload;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardUpload {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // Try to authorize upload via share first
        if try_authorize_upload_via_share(req) {
            return Outcome::Success(GuardUpload);
        }

        // Fall back to JWT cookie authentication (any live user, admin or not)
        match try_jwt_cookie_user(req, &VALIDATION) {
            Ok(_) => return Outcome::Success(GuardUpload),
            Err(err) => {
                let full_err =
                    AppError::from_err(ErrorKind::Auth, err).context("Authentication error");
                Outcome::Error((Status::Unauthorized, full_err))
            }
        }
    }
}
