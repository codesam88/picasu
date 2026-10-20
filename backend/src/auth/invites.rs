//! Single-use registration invitation tokens.
//!
//! An invite is a random 128-bit token shown to its bearer exactly once;
//! only its blake3 hash is stored, keyed by the hash hex, so a leaked
//! database row cannot be replayed as a token. Redemption compares the
//! candidate hash against every stored key with [`subtle::ConstantTimeEq`]
//! and marks the invite used in the same redb write transaction that
//! inserts the new user record (`admin: false`). The password hash lives in
//! `passwd.json` outside that transaction — a crash between the two halves
//! fails safe, because login needs both.

use base64::{Engine as _, engine::general_purpose};
use rand::{TryRng, rngs::SysRng};
use redb::{ReadableDatabase, ReadableTable};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::auth::users::REGISTRATION_CONFLICT_MESSAGE;
use crate::error::{AppError, ErrorKind};
use crate::storage::db::{INVITES, TREE, USERS};

/// Default token lifetime when the creation request names none: 7 days.
pub const DEFAULT_EXPIRY_HOURS: i64 = 7 * 24;
/// Entropy of a fresh token: 128 bits.
const TOKEN_BYTES: usize = 16;
/// Characters of the token kept as a display hint in listings.
const PREFIX_LEN: usize = 8;

/// Stored invite row (JSON value), keyed by the token's blake3 hash hex.
/// Deliberately holds no token material — the prefix is display-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteRecord {
    /// First [`PREFIX_LEN`] characters of the token, for admin display.
    pub prefix: String,
    /// User id of the admin who created the invite.
    pub created_by: String,
    /// Unix seconds after which the invite no longer redeems, or `None`
    /// for an invite that never expires.
    pub expires_at: Option<u64>,
    /// Set once redeemed: the user id that consumed the invite.
    pub used_by: Option<String>,
}

/// Listing view of an invite: the stored metadata plus the row id (the
/// token hash). Never contains the token itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InviteView {
    /// Row id — blake3 hash hex of the token; identifies the invite for
    /// revoke. Preimage-resistant, useless as a token.
    pub id: String,
    pub prefix: String,
    pub created_by: String,
    pub expires_at: Option<u64>,
    pub used_by: Option<String>,
}

impl InviteRecord {
    /// Wrap this record into a listing view under `id` (the token hash).
    #[must_use]
    pub fn into_view(self, id: impl Into<String>) -> InviteView {
        InviteView {
            id: id.into(),
            prefix: self.prefix,
            created_by: self.created_by,
            expires_at: self.expires_at,
            used_by: self.used_by,
        }
    }
}

/// blake3 hash (hex) of `token` — the stored form.
#[must_use]
pub fn hash_token(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn now_secs() -> Result<u64, AppError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| AppError::from_err(ErrorKind::Internal, e.into()))
}

/// Map the creation request's `expires_in_hours` onto a stored deadline:
/// `0` means the invite never expires, positive hours land on that
/// deadline. Negative values are rejected rather than wrapped. The
/// parameter is required — there is no absent-means-default; callers
/// that want the conventional lifetime pass [`DEFAULT_EXPIRY_HOURS`]
/// explicitly.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] for negative hours and
/// [`ErrorKind::Internal`] when the clock is unavailable.
pub fn resolve_expiry(expires_in_hours: i64) -> Result<Option<u64>, AppError> {
    match expires_in_hours {
        0 => Ok(None),
        hours if hours > 0 => Ok(Some(
            now_secs()?.saturating_add(hours.unsigned_abs() * 3600),
        )),
        _ => Err(AppError::new(
            ErrorKind::InvalidInput,
            "expiresInHours must not be negative (0 means never expires)",
        )),
    }
}

/// Create a fresh invite for `created_by` with the conventional 7-day
/// expiry ([`DEFAULT_EXPIRY_HOURS`]).
///
/// Returns the plaintext token (shown once) and the stored record.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] for an empty creator id and
/// [`ErrorKind::Database`]/[`ErrorKind::Internal`] when the clock or the
/// table cannot be used.
pub fn create_invite(created_by: &str) -> Result<(String, InviteRecord), AppError> {
    let expires_at = resolve_expiry(DEFAULT_EXPIRY_HOURS)?;
    create_invite_with_expiry(created_by, expires_at)
}

/// Create an invite with an explicit deadline (`None` never expires).
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] for an empty creator id and
/// [`ErrorKind::Database`] when the table cannot be written.
pub fn create_invite_with_expiry(
    created_by: &str,
    expires_at: Option<u64>,
) -> Result<(String, InviteRecord), AppError> {
    let creator = created_by.trim();
    if creator.is_empty() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "invite creator must not be empty",
        ));
    }
    let mut bytes = [0u8; TOKEN_BYTES];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|e| AppError::from_err(ErrorKind::Internal, e.into()))?;
    let token = general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let record = InviteRecord {
        prefix: token.chars().take(PREFIX_LEN).collect(),
        created_by: creator.to_string(),
        expires_at,
        used_by: None,
    };
    let id = hash_token(&token);
    let json = serde_json::to_string(&record)
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    let txn = TREE
        .in_disk
        .begin_write()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    {
        let mut table = txn
            .open_table(INVITES)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        table
            .insert(id.as_str(), json.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    }
    txn.commit()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    Ok((token, record))
}

/// List all invites (metadata only — never tokens), ordered by row id.
/// A never-created table reads as empty.
///
/// # Errors
/// Returns [`ErrorKind::Database`] when the table cannot be read and
/// [`ErrorKind::Serialization`] when a row is corrupt.
pub fn list_invites() -> Result<Vec<InviteView>, AppError> {
    let txn = TREE
        .in_disk
        .begin_read()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    let table = match txn.open_table(INVITES) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(e) => return Err(AppError::from_err(ErrorKind::Database, e.into())),
    };
    let mut views = Vec::new();
    for entry in table
        .iter()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
    {
        let (key, guard) = entry.map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        let record: InviteRecord = serde_json::from_str(guard.value())
            .map_err(|e| AppError::from_err(ErrorKind::Serialization, e.into()))?;
        views.push(InviteView {
            id: key.value().to_string(),
            prefix: record.prefix,
            created_by: record.created_by,
            expires_at: record.expires_at,
            used_by: record.used_by,
        });
    }
    views.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(views)
}

/// Look up one invite by row id (token hash hex).
///
/// A never-created table reads as `None`.
///
/// # Errors
/// Returns [`ErrorKind::Database`] when the table cannot be read and
/// [`ErrorKind::Serialization`] when the row is corrupt.
pub fn get_invite(invite_id: &str) -> Result<Option<InviteRecord>, AppError> {
    let txn = TREE
        .in_disk
        .begin_read()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    let table = match txn.open_table(INVITES) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(e) => return Err(AppError::from_err(ErrorKind::Database, e.into())),
    };
    match table
        .get(invite_id)
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
    {
        Some(guard) => {
            let record: InviteRecord = serde_json::from_str(guard.value())
                .map_err(|e| AppError::from_err(ErrorKind::Serialization, e.into()))?;
            Ok(Some(record))
        }
        None => Ok(None),
    }
}

/// Revoke an invite by row id: the row is removed, so a later redeem
/// finds nothing. Returns `Ok(false)` when the id is unknown.
///
/// # Errors
/// Returns [`ErrorKind::Database`] when the table cannot be written.
pub fn revoke_invite(invite_id: &str) -> Result<bool, AppError> {
    let txn = TREE
        .in_disk
        .begin_write()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    let removed = {
        let mut table = match txn.open_table(INVITES) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(false),
            Err(e) => return Err(AppError::from_err(ErrorKind::Database, e.into())),
        };
        table
            .remove(invite_id)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
            .is_some()
    };
    txn.commit()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    Ok(removed)
}

/// Redeem `token` for `user_id`: in ONE redb write transaction, verify the
/// token against every stored invite (constant-time compare), mark the
/// invite used, and insert `UserRecord { admin: false, sessions: vec![] }`.
///
/// # Errors
/// - [`ErrorKind::InvalidInput`] — invalid user id
/// - [`ErrorKind::NotFound`] — no invite matches the token (unknown or
///   revoked)
/// - [`ErrorKind::Conflict`] — invite already used, or user already exists
/// - [`ErrorKind::PermissionDenied`] — invite expired
/// - [`ErrorKind::Database`] / [`ErrorKind::Serialization`] — store failure
///
/// The transaction commits or rolls back as a unit: a failed redeem never
/// marks the invite used and never inserts the user.
pub fn redeem_invite(user_id: &str, token: &str) -> Result<(), AppError> {
    let id = crate::auth::users::validate_user_id(user_id)?;
    let candidate = hash_token(token);
    let now = now_secs()?;
    let txn = TREE
        .in_disk
        .begin_write()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    {
        let mut invites = txn
            .open_table(INVITES)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        // Scan every row, comparing hash-to-hash with `subtle` so the
        // decision does not depend on where (or whether) a match occurs.
        let mut any_match = subtle::Choice::from(0);
        let mut found: Option<InviteRecord> = None;
        for entry in invites
            .iter()
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
        {
            let (key, guard) =
                entry.map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
            let eq = candidate.as_bytes().ct_eq(key.value().as_bytes());
            any_match |= eq;
            if bool::from(eq) {
                found = Some(
                    serde_json::from_str(guard.value())
                        .map_err(|e| AppError::from_err(ErrorKind::Serialization, e.into()))?,
                );
            }
        }
        if !bool::from(any_match) {
            return Err(AppError::new(
                ErrorKind::NotFound,
                "invite token is not valid",
            ));
        }
        let mut record =
            found.ok_or_else(|| AppError::new(ErrorKind::NotFound, "invite token is not valid"))?;
        if record.used_by.is_some() {
            return Err(AppError::new(
                ErrorKind::Conflict,
                REGISTRATION_CONFLICT_MESSAGE,
            ));
        }
        if let Some(deadline) = record.expires_at
            && now >= deadline
        {
            return Err(AppError::new(
                ErrorKind::PermissionDenied,
                "invite has expired",
            ));
        }
        let mut users = txn
            .open_table(USERS)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        if users
            .get(id.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
            .is_some()
        {
            return Err(AppError::new(
                ErrorKind::Conflict,
                REGISTRATION_CONFLICT_MESSAGE,
            ));
        }
        record.used_by = Some(id.clone());
        let json = serde_json::to_string(&record)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        invites
            .insert(candidate.as_str(), json.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        let user_json = serde_json::to_string(&crate::auth::users::UserRecord {
            admin: false,
            sessions: vec![],
        })
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        users
            .insert(id.as_str(), user_json.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    }
    txn.commit()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    log::info!("enrollment: invited user {id} registered");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD, reset_backend_state};

    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        guard
    }

    fn raw_row(invite_id: &str) -> Option<String> {
        let txn = TREE.in_disk.begin_read().expect("read txn");
        let table = txn.open_table(INVITES).expect("open INVITES");
        table
            .get(invite_id)
            .expect("get row")
            .map(|g| g.value().to_string())
    }

    #[test]
    fn create_invite_stores_only_hash_and_prefix_hint() {
        let _g = setup();

        let (token, record) = super::create_invite(" s6-root ").expect("create");
        assert_eq!(token.len(), 22, "128-bit base64url token is 22 chars");
        assert_eq!(record.created_by, "s6-root", "creator is trimmed");
        assert!(record.used_by.is_none());
        assert_eq!(
            record.prefix,
            token.chars().take(PREFIX_LEN).collect::<String>()
        );
        let now = super::now_secs().expect("now");
        let expires_at = record.expires_at.expect("default invite expires");
        assert!(
            expires_at >= now + 6 * 24 * 3600 && expires_at <= now + 7 * 24 * 3600 + 60,
            "default expiry must be 7 days from now"
        );
        // The row is keyed by the token's blake3 hash and contains no token
        // material: the stored JSON must not contain the plaintext token.
        let id = super::hash_token(&token);
        assert_eq!(id, blake3::hash(token.as_bytes()).to_hex().to_string());
        let row = raw_row(&id).expect("row must exist");
        assert!(
            !row.contains(&token),
            "stored row must not contain the token"
        );

        reset_backend_state();
    }

    #[test]
    fn create_invite_rejects_empty_creator() {
        let _g = setup();
        let err = super::create_invite("   ").expect_err("empty creator must fail");
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        reset_backend_state();
    }

    #[test]
    fn redeem_marks_used_and_inserts_non_admin_user_atomically() {
        let _g = setup();

        let (token, _) = super::create_invite("s6-root").expect("create");
        super::redeem_invite("s6-reg", &token).expect("redeem");
        assert_eq!(
            crate::auth::users::get_user("s6-reg").expect("get user"),
            Some(crate::auth::users::UserRecord {
                admin: false,
                sessions: vec![]
            })
        );
        let used = super::get_invite(&super::hash_token(&token))
            .expect("get invite")
            .expect("invite exists");
        assert_eq!(used.used_by.as_deref(), Some("s6-reg"));

        // Double use: rejected, and no second user is created.
        let err = super::redeem_invite("s6-reg2", &token).expect_err("double use must fail");
        assert_eq!(err.kind, ErrorKind::Conflict);
        assert_eq!(crate::auth::users::user_count().expect("count"), 1);

        reset_backend_state();
    }

    #[test]
    fn redeem_rejects_expired_invite() {
        let _g = setup();
        let past = super::now_secs().expect("now") - 60;
        let (token, _) = super::create_invite_with_expiry("s6-root", Some(past)).expect("create");
        let err = super::redeem_invite("s6-reg", &token).expect_err("expired must fail");
        assert_eq!(err.kind, ErrorKind::PermissionDenied);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        reset_backend_state();
    }

    #[test]
    fn redeem_rejects_unknown_token() {
        let _g = setup();
        let err =
            super::redeem_invite("s6-reg", "not-a-real-token").expect_err("unknown must fail");
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        reset_backend_state();
    }

    #[test]
    fn redeem_rejects_duplicate_user() {
        let _g = setup();
        crate::auth::users::create_user("s6-taken", false).expect("seed user");
        let (token, _) = super::create_invite("s6-root").expect("create");
        let err = super::redeem_invite("s6-taken", &token).expect_err("duplicate must fail");
        assert_eq!(err.kind, ErrorKind::Conflict);
        // The invite survives the failed redeem for another attempt.
        let rec = super::get_invite(&super::hash_token(&token))
            .expect("get invite")
            .expect("invite exists");
        assert!(rec.used_by.is_none());
        reset_backend_state();
    }

    #[test]
    fn list_reports_metadata_and_never_the_token() {
        let _g = setup();
        let (token, _) = super::create_invite("s6-root").expect("create");
        super::redeem_invite("s6-reg", &token).expect("redeem");

        let listed = super::list_invites().expect("list");
        assert_eq!(listed.len(), 1);
        let view = &listed[0];
        assert_eq!(view.id, super::hash_token(&token));
        assert_eq!(view.created_by, "s6-root");
        assert_eq!(view.used_by.as_deref(), Some("s6-reg"));
        assert!(view.expires_at.expect("listed invite expires") > super::now_secs().expect("now"));
        let json = serde_json::to_string(view).expect("serialize view");
        assert!(
            !json.contains(&token),
            "listing must never expose the token"
        );

        reset_backend_state();
    }

    #[test]
    fn list_on_empty_store_is_empty() {
        let _g = setup();
        assert!(super::list_invites().expect("list").is_empty());
        assert!(super::get_invite("anything").expect("get").is_none());
        reset_backend_state();
    }

    #[test]
    fn revoke_removes_invite_and_blocks_redeem() {
        let _g = setup();
        let (token, _) = super::create_invite("s6-root").expect("create");
        let id = super::hash_token(&token);
        assert!(
            super::revoke_invite(&id).expect("revoke"),
            "existing row removed"
        );
        assert!(
            !super::revoke_invite(&id).expect("revoke again"),
            "unknown id reports false"
        );
        assert!(super::list_invites().expect("list").is_empty());
        let err = super::redeem_invite("s6-reg", &token).expect_err("revoked must fail");
        assert_eq!(err.kind, ErrorKind::NotFound);
        reset_backend_state();
    }

    #[test]
    fn resolve_expiry_maps_hours_to_deadline() {
        let _g = setup();
        let now = super::now_secs().expect("now");
        // Zero means never.
        assert_eq!(super::resolve_expiry(0).expect("zero resolves"), None);
        // Positive hours land on that deadline.
        let two_days = super::resolve_expiry(48)
            .expect("48h resolves")
            .expect("48h expires");
        assert!(
            two_days >= now + 47 * 3600 && two_days <= now + 48 * 3600 + 60,
            "48h deadline, got {two_days}"
        );
        // Negative hours are rejected, not wrapped.
        let err = super::resolve_expiry(-1).expect_err("negative must fail");
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        reset_backend_state();
    }

    #[test]
    fn never_expiring_invite_redeems() {
        let _g = setup();
        let (token, record) = super::create_invite_with_expiry("s6-root", None).expect("create");
        assert!(
            record.expires_at.is_none(),
            "None means the invite never expires"
        );
        super::redeem_invite("s6-reg", &token).expect("never-expiring invite redeems");
        reset_backend_state();
    }
}
