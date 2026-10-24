//! Single-use registration invitation tokens.
//!
//! An invite is a random 128-bit token shown to its bearer exactly once;
//! only its blake3 hash is stored, keyed by the hash hex, so a leaked
//! database row cannot be replayed as a token. Redemption compares the
//! candidate hash against every stored key with [`subtle::ConstantTimeEq`]
//! and marks the invite used in the same redb write transaction. The
//! password hash lives in the user store, outside that transaction — a crash
//! between the two halves fails safe, because login needs both.
//!
//! Invites are memorized on purpose: they are few, and they spend most of
//! their life in the "used or revoked" state, so a list of them has to be
//! maintained either way.

use base64::{Engine as _, engine::general_purpose};
use rand::{TryRng, rngs::SysRng};
use redb::{ReadableDatabase, ReadableTable};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::auth::users::REGISTRATION_CONFLICT_MESSAGE;
use crate::error::{AppError, ErrorKind};
use crate::storage::db::{INVITES, TREE};

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
    /// Optional user id the invite is bound to. When set, only that user
    /// id may redeem it. `#[serde(default)]` keeps old rows (no field)
    /// deserializing to `None`.
    #[serde(default)]
    pub username: Option<String>,
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
    pub username: Option<String>,
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
            username: self.username,
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
    create_invite_with_expiry(created_by, expires_at, None)
}

/// Create an invite with an explicit deadline (`None` never expires).
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] for an empty creator id and
/// [`ErrorKind::Database`] when the table cannot be written.
pub fn create_invite_with_expiry(
    created_by: &str,
    expires_at: Option<u64>,
    username: Option<&str>,
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
    let username = match username {
        Some(u) => {
            let trimmed = u.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(crate::auth::users::validate_user_id(trimmed)?)
            }
        }
        None => None,
    };
    let record = InviteRecord {
        prefix: token.chars().take(PREFIX_LEN).collect(),
        created_by: creator.to_string(),
        expires_at,
        used_by: None,
        username,
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
            username: record.username,
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

/// Redeem `token` for `user_id` with `canonical` (already validated and
/// trimmed password): in ONE redb write transaction, verify the token
/// against every stored invite (constant-time compare) and mark the invite
/// used; then write the identity entry — hash included — to the user store
/// in one read-modify-write.
///
/// Invites stay memorized: they are few, and they spend most of their life
/// in the "used or revoked" state, so a list of them has to be maintained
/// either way. The share tokens stay memorized for the same reason — the
/// admin page lists them and revokes them by id.
///
/// The two stores are separate files, so no single transaction can span
/// them; the order is fixed and fails safe. A crash after the redb
/// commit leaves a consumed invite with no user store entry: registration
/// can be retried only with a fresh invite, and the half-registered id
/// never verifies a password. The reverse order (entry first) would risk
/// a login-capable orphan, so it is deliberately not taken.
///
/// # Errors
/// - [`ErrorKind::InvalidInput`] — invalid user id
/// - [`ErrorKind::NotFound`] — no invite matches the token (unknown or
///   revoked)
/// - [`ErrorKind::Conflict`] — invite already used, or user already exists
/// - [`ErrorKind::PermissionDenied`] — invite expired
/// - [`ErrorKind::Database`] / [`ErrorKind::Serialization`] — store failure
///
/// Each transaction commits or rolls back as a unit: a failed redeem
/// never marks the invite used, never inserts the session record, and
/// never writes the identity entry.
#[allow(clippy::too_many_lines)]
pub fn redeem_invite(user_id: &str, token: &str, canonical: &str) -> Result<(), AppError> {
    let id = crate::auth::users::validate_user_id(user_id)?;
    // Identity lives in the user store; an id already present there is a
    // conflict even if no redb session row exists yet.
    if crate::auth::user_store::read(&crate::auth::users::user_store_path(), &id).is_some() {
        return Err(AppError::new(
            ErrorKind::Conflict,
            REGISTRATION_CONFLICT_MESSAGE,
        ));
    }
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
        // Username binding check first: cheapest, no state change either way.
        if let Some(expected) = record.username.clone()
            && id != expected
        {
            log::warn!("invite username mismatch: bound to {expected}, redeemed as {id}");
            return Err(AppError::new(
                ErrorKind::PermissionDenied,
                "invite is not valid for this user",
            ));
        }
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
        // The identity record is the userdb of record; the check here is an
        // early exit — the store write below re-checks it inside its own
        // transaction.
        if crate::auth::user_store::read(&crate::auth::users::user_store_path(), id.as_str())
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
    }
    txn.commit()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    // Hash between the two store writes: PBKDF2 is CPU-bound and must not
    // sit inside a write transaction. It cannot fail, so placing it after
    // the invite commit widens no failure window — and error paths (expired,
    // unknown, revoked) never pay the cost.
    let hash = crate::auth::password::hash_password(canonical);
    // Second and last store write: the identity entry with its hash, in
    // one rmw. If this fails the invite stays consumed and the entry is
    // absent (fail-safe direction, see the doc comment).
    crate::auth::user_store::rmw(&crate::auth::users::user_store_path(), |users| {
        if users.contains_key(&id) {
            return Err(AppError::new(
                ErrorKind::Conflict,
                REGISTRATION_CONFLICT_MESSAGE,
            ));
        }
        users.insert(
            id.clone(),
            crate::auth::user_store::UserRecord {
                hash: Some(hash),
                admin: false,
                home: None,
                ver: 0,
            },
        );
        Ok(())
    })
    .map_err(|e| AppError::from_err(ErrorKind::IO, e.into()))?;
    log::info!("enrollment: invited user {id} registered");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD, reset_backend_state};
    use crate::tests::fixtures::test_password;

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
        super::redeem_invite("s6-reg", &token, &test_password("s6-reg")).expect("redeem");
        let record = crate::auth::users::user_record("s6-reg")
            .expect("get user")
            .expect("the user must exist");
        assert!(record.hash.is_some(), "the password hash must be stored");
        assert!(!record.admin, "a redeemed invite never grants admin");
        assert_eq!(record.ver, 0);
        let used = super::get_invite(&super::hash_token(&token))
            .expect("get invite")
            .expect("invite exists");
        assert_eq!(used.used_by.as_deref(), Some("s6-reg"));

        // Double use: rejected, and no second user is created.
        let err = super::redeem_invite("s6-reg2", &token, &test_password("s6-reg2"))
            .expect_err("double use must fail");
        assert_eq!(err.kind, ErrorKind::Conflict);
        assert_eq!(crate::auth::users::user_count().expect("count"), 1);

        reset_backend_state();
    }

    /// I5: one redeem leaves a login-capable entry — the hash lands in the
    /// same call, so no caller-side second store write can be skipped or
    /// reordered. The invite being consumed while the entry is missing
    /// (crash between the two stores) stays possible and fails safe; the
    /// entry never exists without the invite having been consumed.
    #[test]
    fn redeem_leaves_login_capable_entry_in_one_call() {
        let _g = setup();

        let (token, _) = super::create_invite("s6-root").expect("create");
        super::redeem_invite("s6-i5", &token, &test_password("s6-i5")).expect("redeem");
        let store =
            crate::auth::user_store::UserStore::load(&crate::auth::users::user_store_path())
                .expect("load store");
        assert!(
            store.verify("s6-i5", &test_password("s6-i5")),
            "the redeemed entry must verify the registered password"
        );
        assert!(!store.verify("s6-i5", &test_password("s6-i5-wrong")));

        reset_backend_state();
    }

    #[test]
    fn s9_pin_invite_cannot_takeover_passwd_only_user() {
        let _g = setup();

        let (token, _) = super::create_invite("s6-root").expect("create");
        let invite_id = super::hash_token(&token);

        // Seed a passwd-only user (identity in passwd store, no redb row).
        let path = crate::auth::users::user_store_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("auth dir");
        }
        crate::auth::user_store::rmw(&path, |users| {
            users.insert(
                "s6-takeover".to_string(),
                crate::auth::user_store::UserRecord {
                    hash: None,
                    admin: false,
                    home: None,
                    ver: 0,
                },
            );
            Ok(())
        })
        .expect("seed passwd-only");

        let err = super::redeem_invite("s6-takeover", &token, &test_password("s6-takeover"))
            .expect_err("passwd-only user must refuse redeem");
        assert_eq!(err.kind, ErrorKind::Conflict);
        assert!(
            err.to_string()
                .contains(crate::auth::users::REGISTRATION_CONFLICT_MESSAGE),
            "uniform conflict message required: {err}"
        );

        // Invite must remain unconsumed.
        let row = raw_row(&invite_id).expect("invite row must exist");
        let rec: super::InviteRecord = serde_json::from_str(&row).expect("parse invite");
        assert!(rec.used_by.is_none(), "invite must not be consumed");

        reset_backend_state();
    }

    #[test]
    fn redeem_rejects_expired_invite() {
        let _g = setup();
        let past = super::now_secs().expect("now") - 60;
        let (token, _) =
            super::create_invite_with_expiry("s6-root", Some(past), None).expect("create");
        let err = super::redeem_invite("s6-reg", &token, &test_password("s6-reg"))
            .expect_err("expired must fail");
        assert_eq!(err.kind, ErrorKind::PermissionDenied);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        reset_backend_state();
    }

    #[test]
    fn redeem_rejects_unknown_token() {
        let _g = setup();
        let err = super::redeem_invite("s6-reg", "not-a-real-token", &test_password("s6-reg"))
            .expect_err("unknown must fail");
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        reset_backend_state();
    }

    #[test]
    fn redeem_rejects_duplicate_user() {
        let _g = setup();
        crate::auth::users::create_user("s6-taken", false).expect("seed user");
        let (token, _) = super::create_invite("s6-root").expect("create");
        let err = super::redeem_invite("s6-taken", &token, &test_password("s6-taken"))
            .expect_err("duplicate must fail");
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
        super::redeem_invite("s6-reg", &token, &test_password("s6-reg")).expect("redeem");

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
        let err = super::redeem_invite("s6-reg", &token, &test_password("s6-reg"))
            .expect_err("revoked must fail");
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
        let (token, record) =
            super::create_invite_with_expiry("s6-root", None, None).expect("create");
        assert!(
            record.expires_at.is_none(),
            "None means the invite never expires"
        );
        super::redeem_invite("s6-reg", &token, &test_password("s6-reg"))
            .expect("never-expiring invite redeems");
        reset_backend_state();
    }

    #[test]
    fn create_invite_with_username_stores_and_lists_binding() {
        let _g = setup();

        let (token, record) =
            super::create_invite_with_expiry("s6-root", None, Some("s6-bound")).expect("create");
        assert_eq!(record.username.as_deref(), Some("s6-bound"));
        let listed = super::list_invites().expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].username.as_deref(), Some("s6-bound"));
        let rec = super::get_invite(&super::hash_token(&token))
            .expect("get")
            .expect("exists");
        assert_eq!(rec.username.as_deref(), Some("s6-bound"));

        reset_backend_state();
    }

    #[test]
    fn redeem_invite_with_matching_username_succeeds() {
        let _g = setup();

        let (token, _) =
            super::create_invite_with_expiry("s6-root", None, Some("s6-match")).expect("create");
        super::redeem_invite("s6-match", &token, &test_password("s6-match")).expect("redeem");
        let used = super::get_invite(&super::hash_token(&token))
            .expect("get")
            .expect("exists");
        assert_eq!(used.used_by.as_deref(), Some("s6-match"));
        assert_eq!(
            crate::auth::users::user_record("s6-match")
                .expect("get user")
                .expect("user exists")
                .hash
                .is_some(),
            true
        );

        reset_backend_state();
    }

    #[test]
    fn redeem_invite_with_mismatched_username_is_403_and_unconsumed() {
        let _g = setup();

        let (token, _) =
            super::create_invite_with_expiry("s6-root", None, Some("s6-expected")).expect("create");
        let invite_id = super::hash_token(&token);
        let err = super::redeem_invite("s6-other", &token, &test_password("s6-other"))
            .expect_err("mismatch must fail");
        assert_eq!(err.kind, ErrorKind::PermissionDenied);
        let rec = super::get_invite(&invite_id).expect("get").expect("exists");
        assert!(rec.used_by.is_none(), "invite must not be consumed");
        assert!(
            crate::auth::users::user_record("s6-other")
                .expect("get")
                .is_none(),
            "no user must be created"
        );

        reset_backend_state();
    }

    #[test]
    fn old_invite_row_without_username_deserializes_to_none() {
        let _g = setup();

        let (token, _) = super::create_invite("s6-root").expect("create");
        let id = super::hash_token(&token);
        let raw = r#"{"prefix":"abcdefgh","createdBy":"s6-root","expiresAt":null,"usedBy":null}"#;
        let txn = crate::storage::db::TREE
            .in_disk
            .begin_write()
            .expect("write txn");
        {
            let mut table = txn
                .open_table(crate::storage::db::INVITES)
                .expect("open INVITES");
            table.insert(id.as_str(), raw).expect("insert");
        }
        txn.commit().expect("commit");

        let rec = super::get_invite(&id).expect("get").expect("exists");
        assert!(rec.username.is_none());

        reset_backend_state();
    }

    #[test]
    fn create_invite_with_invalid_username_is_invalid_input() {
        let _g = setup();

        let long = "a".repeat(65);
        let err = super::create_invite_with_expiry("s6-root", None, Some(&long))
            .expect_err("too-long username must fail");
        assert_eq!(err.kind, ErrorKind::InvalidInput);

        // Empty username normalizes to None rather than erroring.
        let (_, record) =
            super::create_invite_with_expiry("s6-root", None, Some("   ")).expect("empty ok");
        assert!(record.username.is_none());

        reset_backend_state();
    }
}
