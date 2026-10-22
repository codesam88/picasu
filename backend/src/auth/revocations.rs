//! Revocations — the only auth state the server keeps.
//!
//! Tokens are self-contained capabilities: everything a request needs is
//! inside the token, and the server keeps no session list. What it does
//! keep is the list of capabilities that must not work any more: a session
//! that was logged out, an invite that was redeemed or revoked, a share
//! that was disabled. Each revocation carries the expiry of the thing it
//! revokes, so it can be forgotten when that expires.
//!
//! The records live in the index database (`revocations` table): they are
//! runtime state, not identity, and the index database is the file that
//! holds transient state.

use redb::{ReadableDatabase as _, ReadableTable as _};

use crate::error::{AppError, ErrorKind};
use crate::storage::db::{REVOCATIONS, TREE};

/// A revoked capability: the key it is filed under and the moment it stops
/// mattering.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Revocation {
    /// Unix seconds after which the revocation can be forgotten: the expiry
    /// of the token, invite or share it revokes.
    pub expires_at: u64,
}

/// Key of a revoked session token.
#[must_use]
pub fn session_key(jti: &str) -> String {
    format!("session:{jti}")
}

/// Key of a revoked invitation token.
#[must_use]
pub fn invite_key(invite_id: &str) -> String {
    format!("invite:{invite_id}")
}

/// Key of a revoked share.
#[must_use]
pub fn share_key(share_id: &str) -> String {
    format!("share:{share_id}")
}

fn now_secs() -> u64 {
    #[allow(clippy::cast_sign_loss)]
    {
        (chrono::Utc::now().timestamp_millis() / 1000) as u64
    }
}

/// Revoke the capability filed under `key` until `expires_at`. Revoking an
/// already revoked key refreshes its expiry.
///
/// Expired revocations are dropped in the same transaction: the set only
/// ever holds what still matters.
///
/// # Errors
/// Returns [`ErrorKind::Database`] when the index database cannot be
/// written.
pub fn revoke(key: &str, expires_at: u64) -> Result<(), AppError> {
    let txn = TREE
        .in_disk
        .begin_write()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    {
        let mut table = txn
            .open_table(REVOCATIONS)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        let now = now_secs();
        let expired: Vec<String> = table
            .iter()
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
            .filter_map(std::result::Result::ok)
            .filter_map(|(id, value)| {
                let value: Revocation = serde_json::from_str(value.value()).ok()?;
                (value.expires_at <= now).then(|| id.value().to_string())
            })
            .collect();
        for id in expired {
            table
                .remove(id.as_str())
                .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        }
        let value = serde_json::to_string(&Revocation { expires_at })
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        table
            .insert(key, value.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    }
    txn.commit()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    Ok(())
}

/// Whether the capability filed under `key` is revoked. An expired
/// revocation no longer counts: the capability it revokes is expired too.
#[must_use]
pub fn is_revoked(key: &str) -> bool {
    let Ok(txn) = TREE.in_disk.begin_read() else {
        // A database that cannot be read must not grant access.
        return true;
    };
    let Ok(table) = txn.open_table(REVOCATIONS) else {
        return false;
    };
    let Ok(Some(value)) = table.get(key) else {
        return false;
    };
    let Ok(revocation) = serde_json::from_str::<Revocation>(value.value()) else {
        return true;
    };
    revocation.expires_at > now_secs()
}

/// Drop every revocation. Used when the signing secret is rotated: the old
/// tokens are dead anyway, and their revocations no longer say anything.
///
/// # Errors
/// Returns [`ErrorKind::Database`] when the index database cannot be
/// written.
pub fn clear() -> Result<(), AppError> {
    let txn = TREE
        .in_disk
        .begin_write()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    {
        let mut table = txn
            .open_table(REVOCATIONS)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        let keys: Vec<String> = table
            .iter()
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
            .filter_map(std::result::Result::ok)
            .map(|(id, _)| id.value().to_string())
            .collect();
        for key in keys {
            table
                .remove(key.as_str())
                .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        }
    }
    txn.commit()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::bootstrap::{lock, setup};

    fn key(name: &str) -> String {
        format!("test-revocation:{name}")
    }

    #[test]
    fn a_revoked_key_reads_as_revoked() {
        let _g = lock();
        let _client = setup();
        let k = key("revoked");
        revoke(&k, now_secs() + 3600).expect("revoke");
        assert!(is_revoked(&k));
    }

    #[test]
    fn an_unknown_key_reads_as_not_revoked() {
        let _g = lock();
        let _client = setup();
        assert!(!is_revoked(&key("unknown")));
    }

    #[test]
    fn an_expired_revocation_no_longer_counts() {
        let _g = lock();
        let _client = setup();
        let k = key("expired");
        revoke(&k, now_secs() - 1).expect("revoke");
        assert!(
            !is_revoked(&k),
            "a revocation that outlived its capability must not deny it"
        );
    }

    #[test]
    fn keys_are_namespaced_by_kind() {
        assert_eq!(session_key("abc"), "session:abc");
        assert_eq!(invite_key("abc"), "invite:abc");
        assert_eq!(share_key("abc"), "share:abc");
    }
}
