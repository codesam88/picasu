#![allow(clippy::module_inception)]

use crate::model::album::ResolvedShare;
#[allow(unused_imports)]
use crate::model::album::Share;
use crate::model::config::APP_CONFIG;
use crate::router::{AppResult, GuardError, GuardResult};
use chrono::Utc;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Admin,
    Share(Box<ResolvedShare>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claims {
    pub role: Role,
    pub exp: u64, // seconds since epoch
}

impl Claims {
    pub fn new_admin() -> Self {
        #[allow(clippy::cast_sign_loss)]
        let exp = (Utc::now().timestamp_millis() / 1000) as u64 + 14 * 86_400; // 14 days

        Self {
            role: Role::Admin,
            exp,
        }
    }

    pub fn new_share(resolved_share: ResolvedShare) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let exp = (Utc::now().timestamp_millis() / 1000) as u64 + 14 * 86_400; // 14 days

        Self {
            role: Role::Share(Box::new(resolved_share)),
            exp,
        }
    }
    pub fn is_admin(&self) -> bool {
        matches!(self.role, Role::Admin)
    }
    pub fn get_share(self) -> Option<ResolvedShare> {
        match self.role {
            Role::Share(share) => Some(*share),
            Role::Admin => None,
        }
    }

    pub fn encode(&self) -> String {
        use crate::model::config::APP_CONFIG;

        let config = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned");
        self.encode_with_key(&config.get_jwt_secret_key())
    }

    pub fn encode_with_key(&self, key: &[u8]) -> String {
        encode(&Header::default(), &self, &EncodingKey::from_secret(key))
            .expect("Failed to generate token")
    }
}

// src/router/claims/claims_hash.rs
use arrayvec::ArrayString;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimsHash {
    pub allow_original: bool,
    pub hash: ArrayString<64>,
    pub timestamp: i64,
    pub exp: u64,
}

impl ClaimsHash {
    pub fn new(hash: ArrayString<64>, timestamp: i64, allow_original: bool) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let exp = (Utc::now().timestamp_millis() / 1000) as u64 + 300;

        Self {
            allow_original,
            hash,
            timestamp,
            exp,
        }
    }

    pub fn encode(&self) -> String {
        let secret_key = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned")
            .get_jwt_secret_key();
        encode(
            &Header::default(),
            &self,
            &EncodingKey::from_secret(&secret_key),
        )
        .expect("Failed to generate token")
    }
}

// src/router/claims/claims_timestamp.rs

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimsTimestamp {
    pub resolved_share_opt: Option<ResolvedShare>,
    pub timestamp: i64,
    pub exp: u64,
}

impl ClaimsTimestamp {
    pub fn new(resolved_share_opt: Option<ResolvedShare>, timestamp: i64) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let exp = (Utc::now().timestamp_millis() / 1000) as u64 + 300;

        Self {
            resolved_share_opt,
            timestamp,
            exp,
        }
    }

    pub fn encode(&self) -> String {
        let secret_key = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned")
            .get_jwt_secret_key();
        encode(
            &Header::default(),
            &self,
            &EncodingKey::from_secret(&secret_key),
        )
        .expect("Failed to generate token")
    }
}

use std::sync::LazyLock;

use jsonwebtoken::{Algorithm, Validation};
use rocket::Route;

pub fn generate_fairing_routes() -> Vec<Route> {
    routes![renew_timestamp_token, renew_hash_token]
}

static VALIDATION: LazyLock<Validation> = LazyLock::new(|| Validation::new(Algorithm::HS256));

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
}

// src/router/fairing/auth_utils.rs
use crate::error::{AppError, ErrorKind};
use crate::model::abstract_data::AbstractData;
use crate::storage::db::DATA_TABLE;
use crate::storage::db::TREE;

use anyhow::{Error, Result, anyhow};
use jsonwebtoken::{DecodingKey, decode};
use log::info;
use redb::ReadableDatabase;
use rocket::Request;
use serde::de::DeserializeOwned;

// Error types for share validation are now handled by AppError

/// Extract and validate Authorization header Bearer token
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

    if let Some(Ok(token)) = req.query_value::<&str>("token") {
        return Ok(token);
    }

    Err(anyhow!(
        "Request is missing the Authorization header or token query parameter"
    ))
}

/// Decode JWT token with given claims type and validation
pub fn my_decode_token<T: DeserializeOwned>(token: &str, validation: &Validation) -> Result<T> {
    let secret_key = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned")
        .get_jwt_secret_key();

    match decode::<T>(token, &DecodingKey::from_secret(&secret_key), validation) {
        Ok(token_data) => Ok(token_data.claims),
        Err(err) => {
            info!("Token decode failed: {err:?}");
            Err(Error::from(err).context("Failed to decode JWT token"))
        }
    }
}

/// Try to authenticate via JWT cookie and check if user is admin
pub fn try_jwt_cookie_auth(req: &Request<'_>, validation: &Validation) -> Result<Claims> {
    // If no password is set, allow access as admin
    if APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned")
        .password
        .is_none()
    {
        return Ok(Claims::new_admin());
    }

    if let Some(jwt_cookie) = req.cookies().get("jwt") {
        let token = jwt_cookie.value();
        let claims = my_decode_token::<Claims>(token, validation)?;
        if claims.is_admin() {
            return Ok(claims);
        }
        return Err(anyhow!("User is not an admin"));
    }
    Err(anyhow!("JWT not found in cookies"))
}

/// Extract hash from the request URL path (last segment before extension)
pub fn extract_hash_from_path(req: &Request<'_>) -> Result<String> {
    let hash_opt = req
        .uri()
        .path()
        .segments()
        .last()
        .and_then(|hash_with_ext| hash_with_ext.rsplit_once('.'))
        .map(|(hash, _ext)| hash.to_string());

    match hash_opt {
        Some(hash) => Ok(hash),
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
            && header_pwd == pwd
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

fn resolve_share_internal(
    album_id: &str,
    share_id: &str,
    req: &Request<'_>,
) -> Result<Option<Claims>, AppError> {
    let read_txn = TREE.in_disk.begin_read().map_err(|e| {
        AppError::from_err(ErrorKind::Database, e.into())
            .context("Failed to begin read transaction")
    })?;

    let table = read_txn.open_table(DATA_TABLE).map_err(|e| {
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

    let abstract_data = data_guard.value();
    let AbstractData::Album(mut album) = abstract_data else {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!("Data with id '{album_id}' is not an album"),
        ));
    };

    let share = album.metadata.share_list.remove(share_id).ok_or_else(|| {
        AppError::new(
            ErrorKind::NotFound,
            format!("Share '{share_id}' not found in album '{album_id}'"),
        )
    })?;

    // Validate share access (password and expiration)
    validate_share_access(&share, req)?;

    let resolved_share = ResolvedShare::new(
        ArrayString::<64>::from(album_id)
            .map_err(|_| AppError::new(ErrorKind::Internal, "Failed to parse album_id"))?,
        album.metadata.title,
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
        && let Ok(table) = read_txn.open_table(DATA_TABLE)
        && let Ok(Some(data_guard)) = table.get(album_id)
        && let AbstractData::Album(mut album) = data_guard.value()
        && let Some(share) = album.metadata.share_list.remove(share_id)
        && share.show_upload
        && validate_share_access(&share, req).is_ok()
        && let Some(Ok(album_id_parsed)) = req.query_value::<&str>("presigned_album_id_opt")
    {
        return album.object.id.as_str() == album_id_parsed;
    }

    false
}

// src/router/fairing/guard_auth.rs
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};

pub struct GuardAuth;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardAuth {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match try_jwt_cookie_auth(req, &VALIDATION) {
            Ok(_) => Outcome::Success(GuardAuth),
            Err(err) => Outcome::Error((
                Status::Unauthorized,
                AppError::from_err(ErrorKind::Auth, err).context("Authentication error"),
            )),
        }
    }
}

// src/router/fairing/guard_hash.rs
use log::warn;
use rocket::serde::json::Json;

use crate::error::ResultExt;

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

        let claims: ClaimsHash = match my_decode_token(token, &VALIDATION) {
            Ok(claims) => claims,
            Err(err) => {
                return Outcome::Error((
                    Status::Unauthorized,
                    AppError::from_err(ErrorKind::Auth, err).context("JWT decoding failed"),
                ));
            }
        };

        let data_hash = match extract_hash_from_path(req) {
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

        let claims: ClaimsHash = match my_decode_token(token, &VALIDATION) {
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
            return Outcome::Forward(Status::Unauthorized);
        }

        let data_hash = match extract_hash_from_path(req) {
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

#[utoipa::path(
        post,
        path = "/post/renew-hash-token",
        request_body = RenewHashToken,
        responses(
            (status = 200, description = "Hash token renewed", body = RenewHashTokenReturn),
            (status = 400, description = "Invalid input"),
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
        let token_data = match decode::<ClaimsHash>(
            &expired_hash_token,
            &DecodingKey::from_secret(
                &APP_CONFIG
                    .get()
                    .expect("APP_CONFIG not initialized")
                    .read()
                    .expect("lock poisoned")
                    .get_jwt_secret_key(),
            ),
            &VALIDATION_ALLOW_EXPIRED,
        ) {
            Ok(data) => data,
            Err(err) => {
                warn!("Token renewal failed: unable to decode token. Error: {err:#?}");
                return Err(AppError::new(
                    ErrorKind::Auth,
                    "Unauthorized: Invalid token",
                ));
            }
        };

        if token_data.claims.timestamp != auth.timestamp_decoded {
            warn!(
                "Timestamp does not match. Received: {}, Expected: {}",
                token_data.claims.timestamp, auth.timestamp_decoded
            );
            return Err(AppError::new(
                ErrorKind::Auth,
                "Unauthorized: Timestamp mismatch",
            ));
        }

        let claims = token_data.claims;
        let new_hash_claims = ClaimsHash::new(claims.hash, claims.timestamp, claims.allow_original);
        let new_hash_token = new_hash_claims.encode();

        Ok(Json(RenewHashTokenReturn {
            token: new_hash_token,
        }))
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?
}

pub struct TimestampGuardModified {
    pub timestamp_decoded: i64,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for TimestampGuardModified {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let Ok(token) = extract_bearer_token(req) else {
            // Note: We can't access `err` easily with let-else if we just unwrap Ok.
            // But extract_bearer_token returns Result.
            // To preserve error message we need the match, or be clever.
            // Clippy suggestion was `let Ok(token) = ... else { ... }`
            // But we need to use `err` in the `Outcome::Error`.
            // Wait, if we use let-else, we lose the error variable unless we match it out.
            // `let Ok(token) = ...` destructures Ok.
            // If it is Err(err), the else block executes, but we don't have access to `err`.
            // So for these cases, `manual_let_else` might be WRONG if we need the error value.
            // But let's look at the clippy warning again.
            // It says "this could be rewritten as `let...else`".
            // If I rewrite it, I might lose the error context unless I re-extract it or use a different pattern.
            // Actually, if I use `let Ok(token) = ...` I can't get the error.
            // So I will ignore this specific instance if I need the error.
            // BUT, wait, for `GuardHash`, line 200: `let token = match extract_bearer_token(req) { Ok(token) => token, Err(_) => return Outcome::Forward(Status::Unauthorized) };`
            // That one (line 200) ignores the error! So that one IS valid for let-else.
            // Lines 27-36 USE the error. So clippy shouldn't be complaining about those?
            // Let's re-read the clippy output.
            // Warning at `src/router/fairing/guard_hash.rs:200:9` -> This is inside `TimestampGuardModified`.
            // Warning for `GuardHash` itself? I don't see it in the snippet I posted in thought block.
            // Ah, I see "warning: this could be rewritten as `let...else` --> src/router/fairing/guard_hash.rs:200:9".
            // Only line 200 is complained about!
            // Line 27 was NOT complained about in the log I read.
            // Okay, so I only fix line 200.
            return match extract_bearer_token(req) {
                Ok(token) => match my_decode_token::<ClaimsTimestamp>(token, &VALIDATION) {
                    Ok(claims) => Outcome::Success(TimestampGuardModified {
                        timestamp_decoded: claims.timestamp,
                    }),
                    Err(_) => Outcome::Forward(Status::Unauthorized),
                },
                Err(_) => Outcome::Forward(Status::Unauthorized),
            };
        };

        // Wait, the code at 200 is:
        /*
        let token = match extract_bearer_token(req) {
            Ok(token) => token,
            Err(_) => return Outcome::Forward(Status::Unauthorized),
        };
        */
        // I will change it to let-else.
        let Ok(_token) = extract_bearer_token(req) else {
            return Outcome::Forward(Status::Unauthorized);
        };

        let Ok(claims) = my_decode_token::<ClaimsTimestamp>(token, &VALIDATION) else {
            return Outcome::Forward(Status::Unauthorized);
        };

        Outcome::Success(TimestampGuardModified {
            timestamp_decoded: claims.timestamp,
        })
    }
}

// src/router/fairing/guard_read_only_mode.rs

pub struct GuardReadOnlyMode;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardReadOnlyMode {
    type Error = GuardError;
    async fn from_request(_req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        if APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned")
            .read_only_mode
        {
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

        // Fall back to JWT cookie authentication (Admin)
        match try_jwt_cookie_auth(req, &VALIDATION) {
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

        let claims: ClaimsTimestamp = match my_decode_token(token, &VALIDATION) {
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

#[utoipa::path(
        post,
        path = "/post/renew-timestamp-token",
        request_body = RenewTimestampToken,
        responses(
            (status = 200, description = "Timestamp token renewed", body = RenewTimestampTokenReturn),
            (status = 400, description = "Invalid input"),
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
    let _ = auth?;
    tokio::task::spawn_blocking(move || {
        let token = token_request.into_inner().token;
        let token_data = match decode::<ClaimsTimestamp>(
            &token,
            &DecodingKey::from_secret(
                &APP_CONFIG
                    .get()
                    .expect("APP_CONFIG not initialized")
                    .read()
                    .expect("lock poisoned")
                    .get_jwt_secret_key(),
            ),
            &VALIDATION_ALLOW_EXPIRED,
        ) {
            Ok(data) => data,
            Err(err) => {
                warn!("Token renewal failed: unable to decode token, error: {err:#?}");
                return Err(AppError::new(
                    ErrorKind::Auth,
                    "Unauthorized: Invalid token",
                ));
            }
        };

        let claims = token_data.claims;
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

        // Fall back to JWT cookie authentication
        match try_jwt_cookie_auth(req, &VALIDATION) {
            Ok(_) => return Outcome::Success(GuardUpload),
            Err(err) => {
                let full_err =
                    AppError::from_err(ErrorKind::Auth, err).context("Authentication error");
                Outcome::Error((Status::Unauthorized, full_err))
            }
        }
    }
}
