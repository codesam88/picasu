//! The write-sidecar-then-store order every metadata edit shares.
//!
//! The file plus its sidecar is the source of truth; `METADATA_TABLE` is a
//! cache of it. So an edit only *happened* once its sidecar is written, and the
//! two must never be allowed to disagree in the direction that hides a failure
//! — a cached edit that is in no file is a silent lie that the next reindex
//! reverts, with nothing in the meantime to say so.
//!
//! [`commit_metadata_edits`] is the whole of that contract, and every endpoint
//! that edits metadata goes through it: `/put/edit_tag`,
//! `/put/set_user_defined_description`, `/put/edit_rating` and
//! `/put/set_album_title` (whose album-sidecar half of the picture is
//! `.albuminfo.xmp`).
//!
//! # What a failed write leaves behind
//!
//! **Sidecar ahead of the cache converges; the other way round does not.** A
//! payload whose store fails after its sidecar landed leaves the file holding an
//! edit the cache does not have, and the next reindex reads the file and adopts
//! it — the user's edit survives, nothing is lost, and no rollback is attempted
//! for it. The reverse, a cache holding an edit no file has, is silent until the
//! reindex that erases it, which is the failure this module exists to prevent.
//! So the store phase is not undone when it fails, and the write phase is
//! undone when it does.
//!
//! # Residuals
//!
//! * A rollback that itself fails leaves that one sidecar ahead of the cache. It
//!   is logged at error level, one line per file, with no rate limit: the
//!   request is failing anyway and an operator reading the log is the only one
//!   who can fix the directory, and a deduplicated line would be scrolled away
//!   by the very log they would be reading. The next reindex converges it.
//! * Two edits of the *same* asset in flight at once can interleave: the second
//!   one's backup is the first one's write, and a rollback restores that. The
//!   result is a sidecar holding one of the two edits, which is what the last
//!   writer asked for. Serialising edits per asset is not this module's job.

use crate::error::{AppError, ErrorKind};
use crate::model::abstract_data::AbstractData;
use crate::process::xmp_write::{sidecar_path, write_bytes_atomically, write_sidecar_for};
use anyhow::Result;
use arrayvec::ArrayString;
use std::io;
use std::path::PathBuf;

/// One item an edit request touched: the id its payload is stored under, and
/// the composed view the edit was applied to.
pub struct EditedItem {
    pub asset_id: ArrayString<64>,
    pub data: AbstractData,
}

/// Write every item's sidecar, and only then store every payload.
///
/// `store` is the caller's own write into its own transaction — `edit_album`
/// holds a `redb` transaction open across this call and inserts through it,
/// the photo endpoints call `store_metadata_record` — because the store phase
/// is where they differ and the ordering here is what they must not.
///
/// # Errors
///
/// Fails the request if any sidecar could not be written or read, after putting
/// back every sidecar this call had already written, so nothing is stored: an
/// item whose file does not carry the edit is not an edit, and the cache must
/// not claim it is. The error is [`ErrorKind::IO`] — the filesystem refused the
/// write, which is a server-side condition and not a malformed request, so it
/// renders as a 500 rather than as the 400 or 403 a client mistake would get.
pub fn commit_metadata_edits(
    items: &[EditedItem],
    mut store: impl FnMut(&ArrayString<64>, &AbstractData) -> Result<(), AppError>,
) -> Result<(), AppError> {
    commit_with(items, &mut store, &mut write_sidecar_for)
}

fn commit_with(
    items: &[EditedItem],
    store: &mut impl FnMut(&ArrayString<64>, &AbstractData) -> Result<(), AppError>,
    write: &mut impl FnMut(&AbstractData) -> io::Result<()>,
) -> Result<(), AppError> {
    // Only the writes that landed. The item that failed is not among them: a
    // write that reports failure leaves its file as it was (ExifTool stages
    // beside the target and renames on success, as does the managed-packet
    // fallback), and restoring a file that never changed would turn the ordinary
    // read-only-directory failure into a spurious "rollback failed" for a file
    // that is still correct.
    let mut applied: Vec<SidecarBackup> = Vec::new();
    for item in items {
        // A capture failure is a write failure with one step's delay: by the
        // time it is found, this request has already written earlier items, and
        // they are the same divergence the write branch rolls back.
        let backup = match SidecarBackup::capture(item) {
            Ok(Some(backup)) => backup,
            // An item with no path has no sidecar to write and none to undo, so
            // it goes straight to the store phase like any other.
            Ok(None) => continue,
            Err(err) => {
                rollback(&applied);
                return Err(err);
            }
        };
        if let Err(err) = write(&item.data) {
            rollback(&applied);
            return Err(
                AppError::from_err(ErrorKind::IO, err.into()).context(format!(
                    "the edit of asset {} was not written to {} and has not been stored",
                    item.asset_id,
                    backup.path.display()
                )),
            );
        }
        applied.push(backup);
    }

    for item in items {
        store(&item.asset_id, &item.data)?;
    }
    Ok(())
}

/// The bytes a sidecar held immediately before this request wrote it.
struct SidecarBackup {
    path: PathBuf,
    /// `None` when the sidecar did not exist, which is what makes undoing the
    /// write a removal rather than a restore.
    previous: Option<Vec<u8>>,
}

impl SidecarBackup {
    /// Read what `item`'s sidecar holds now, or `None` when the item has no
    /// sidecar at all.
    ///
    /// A sidecar that cannot be *read* fails the request rather than being
    /// written blind: without the previous bytes there is no way to undo the
    /// write, and a write that cannot be undone is not one this contract can
    /// make.
    fn capture(item: &EditedItem) -> Result<Option<Self>, AppError> {
        let Some(path) = sidecar_path(&item.data) else {
            return Ok(None);
        };
        let previous = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            // Absent is an answer, not a failure: the write will create the file
            // and the undo has to remove it again.
            Err(err) if err.kind() == io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(
                    AppError::from_err(ErrorKind::IO, err.into()).context(format!(
                        "the previous {} could not be read, so the edit of asset {} cannot be \
                     written safely",
                        path.display(),
                        item.asset_id
                    )),
                );
            }
        };
        Ok(Some(Self { path, previous }))
    }

    /// Put the sidecar back the way this request found it.
    fn restore(&self) -> io::Result<()> {
        match &self.previous {
            Some(bytes) => write_bytes_atomically(&self.path, bytes),
            // The request created this file, so undoing the request removes it.
            // A file that is already gone is the state the undo wants.
            None => match std::fs::remove_file(&self.path) {
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        }
    }
}

/// Put every written sidecar back, reporting the ones that could not be.
///
/// Best-effort and loud: the request is already failing, so this is the last
/// chance to tell an operator that a file now disagrees with the cache in the
/// direction only a reindex can fix.
fn rollback(applied: &[SidecarBackup]) {
    for backup in applied.iter().rev() {
        if let Err(err) = backup.restore() {
            log::error!(
                "rolling back the XMP sidecar {} failed: {err}; it now holds an edit the cache \
                 does not have, and the next reindex is what will reconcile them",
                backup.path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    /// Bytes a seeded sidecar is given, distinguishable from anything the fake
    /// write in these tests produces.
    const SEED: &[u8] = b"<?xpacket begin='' id='seed'?><dc:subject>seeded</dc:subject>";

    /// What the fake write leaves in a sidecar it "writes".
    const WRITTEN: &[u8] = b"<?xpacket begin='' id='written'?><dc:subject>edited</dc:subject>";

    /// A photo at `dir/file`, so the sidecar the production path derives is
    /// `dir/file.xmp` — the same file these tests read back.
    fn photo(dir: &Path, file: &str, tags: &[&str]) -> AbstractData {
        let mut metadata = ImageMetadata::new(0, 4, 3, "jpg".to_string());
        metadata.path = Some(crate::model::response::FileEntry::new(&dir.join(file), 0));
        let mut object = ObjectSchema::new(
            ArrayString::from("img").expect("hash fits"),
            ObjectType::Image,
        );
        object.tags = tags.iter().map(|t| (*t).to_string()).collect();
        AbstractData::Image(ImageCombined { object, metadata })
    }

    fn item(id: &str, data: AbstractData) -> EditedItem {
        EditedItem {
            asset_id: ArrayString::from(id).expect("id fits"),
            data,
        }
    }

    fn sidecar_of(data: &AbstractData) -> PathBuf {
        let file = data.path().expect("a pathed photo has a path");
        Path::new(&file.file).with_extension("xmp")
    }

    /// Two items in one directory, so a request over both is one rollback away
    /// from a partial edit.
    fn two_items(dir: &Path) -> Vec<EditedItem> {
        vec![
            item("one", photo(dir, "first.jpg", &["alpha"])),
            item("two", photo(dir, "second.jpg", &["beta"])),
        ]
    }

    /// A write that lands for the first item and fails for the second, which
    /// is the shape a batch rollback has to exist for: the first item's file
    /// is already edited by the time the second one's cannot be.
    fn write_second_fails(first: &Path) -> impl FnMut(&AbstractData) -> io::Result<()> {
        let mut calls = 0;
        move |_data: &AbstractData| {
            calls += 1;
            if calls == 1 {
                std::fs::write(first, WRITTEN).map_err(io::Error::other)
            } else {
                Err(io::Error::other("injected sidecar write failure"))
            }
        }
    }

    /// The control: a request whose every sidecar write lands stores every
    /// payload, and stores them after the writes rather than before.
    ///
    /// Without this, the other tests' "nothing was stored" assertions would
    /// also pass for an implementation that never stores anything.
    #[test]
    fn a_request_whose_writes_all_land_stores_every_payload_after_them() {
        let dir = tempfile::tempdir().expect("temp dir");
        let items = two_items(dir.path());
        // Both phases log into one sequence, so the assertion is about their
        // order rather than about two lists that could each be right alone.
        let phases: RefCell<Vec<String>> = RefCell::new(Vec::new());

        commit_with(
            &items,
            &mut |id: &ArrayString<64>, _data: &AbstractData| {
                phases.borrow_mut().push(format!("store:{id}"));
                Ok(())
            },
            &mut |data: &AbstractData| {
                let sidecar = sidecar_of(data);
                phases
                    .borrow_mut()
                    .push(format!("write:{}", sidecar.display()));
                std::fs::write(sidecar, WRITTEN).map_err(io::Error::other)
            },
        )
        .expect("every write lands");

        assert_eq!(
            *phases.borrow(),
            vec![
                format!("write:{}", dir.path().join("first.xmp").display()),
                format!("write:{}", dir.path().join("second.xmp").display()),
                "store:one".to_string(),
                "store:two".to_string(),
            ],
            "every sidecar is written before the first payload is stored"
        );
        assert_eq!(
            std::fs::read(dir.path().join("first.xmp")).expect("written"),
            WRITTEN,
            "the writes are not a no-op: the premise of the phase order holds"
        );
    }

    /// The failure the whole module exists for: an edit whose sidecar could
    /// not be written is not an edit, so the request fails and the cache keeps
    /// what it had.
    ///
    /// The error kind is the only part of this the client sees, so it is
    /// pinned: `IO` renders as a server-side I/O failure, which is what
    /// refused the write. `InvalidInput` and `PermissionDenied` would blame a
    /// request that is perfectly well formed on a directory the server owns,
    /// and `Internal` would report a broken server rather than a filesystem
    /// that did not accept the bytes.
    #[test]
    fn a_failed_sidecar_write_fails_the_request_and_reaches_no_store_phase() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("first.xmp");
        let items = two_items(dir.path());
        let mut stored: Vec<String> = Vec::new();

        let outcome = commit_with(
            &items,
            &mut |id: &ArrayString<64>, _data: &AbstractData| {
                stored.push(id.to_string());
                Ok(())
            },
            &mut write_second_fails(&first),
        );

        let err = outcome.expect_err("a failed sidecar write must fail the request");
        assert_eq!(
            err.kind,
            crate::error::ErrorKind::IO,
            "the write failed on the filesystem, and is reported as that"
        );
        assert!(
            stored.is_empty(),
            "the store phase must not run after a failed write, stored: {stored:?}"
        );
    }

    /// A rollback has to put back the sidecar a *previous* write overwrote, not
    /// only the one that failed: the first item's edit is applied to its file
    /// before the second item's write is even attempted, so the file and the
    /// cache would otherwise disagree about the first item alone.
    #[test]
    fn a_failed_write_restores_the_bytes_a_previous_sidecar_had() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("first.xmp");
        std::fs::write(&first, SEED).expect("seed the sidecar the first write overwrites");
        let items = two_items(dir.path());

        commit_with(
            &items,
            &mut |_id: &ArrayString<64>, _data: &AbstractData| Ok(()),
            &mut write_second_fails(&first),
        )
        .expect_err("the request fails");

        assert_eq!(
            std::fs::read(&first).expect("the sidecar is still there"),
            SEED,
            "the overwritten sidecar must be byte-identical to what it was"
        );
    }

    /// The other half of the rollback: an edit that *created* a sidecar has to
    /// take it away again, or a failed request leaves a file on disk
    /// describing an edit no cache holds.
    #[test]
    fn a_failed_write_removes_a_sidecar_that_did_not_exist_before() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("first.xmp");
        let items = two_items(dir.path());
        assert!(!first.exists(), "the premise: there is no sidecar yet");

        commit_with(
            &items,
            &mut |_id: &ArrayString<64>, _data: &AbstractData| Ok(()),
            &mut write_second_fails(&first),
        )
        .expect_err("the request fails");

        assert!(
            !first.exists(),
            "a sidecar this request created must not outlive it"
        );
    }

    /// An item with no file has no sidecar to write and none to undo, so it is
    /// neither a write failure nor a rollback casualty: its payload still
    /// reaches the store phase, and a failure elsewhere in the request still
    /// keeps it out.
    #[test]
    fn an_item_with_no_file_is_stored_and_never_rolled_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("first.xmp");
        // A pruned record: `ImageMetadata::new` leaves no path, which is what
        // `AbstractData::path` reports for it.
        let pathless = {
            let mut metadata = ImageMetadata::new(0, 4, 3, "jpg".to_string());
            metadata.path = None;
            let object = ObjectSchema::new(
                ArrayString::from("img").expect("hash fits"),
                ObjectType::Image,
            );
            AbstractData::Image(ImageCombined { object, metadata })
        };
        let items = vec![
            item("one", photo(dir.path(), "first.jpg", &["alpha"])),
            item("pruned", pathless),
        ];
        let mut stored: Vec<String> = Vec::new();

        commit_with(
            &items,
            &mut |id: &ArrayString<64>, _data: &AbstractData| {
                stored.push(id.to_string());
                Ok(())
            },
            &mut |_data: &AbstractData| Ok(()),
        )
        .expect("an item with no sidecar is not a write failure");

        assert_eq!(stored, vec!["one", "pruned"]);
        assert!(
            !first.exists(),
            "nothing was written, so nothing had to be taken back"
        );
    }

    /// A sidecar that cannot be *read* fails the request in the middle of a
    /// batch, and it fails it the same way a write does: everything this
    /// request had already written goes back.
    ///
    /// The unreadable file is deliberately the *second* item's, so the first
    /// one's write is already in its file when the failure is found. A capture
    /// failure that returned without rolling back would leave that file
    /// describing an edit nothing stored — the divergence this module exists to
    /// prevent, reached through the read side rather than the write side.
    #[cfg(unix)]
    #[test]
    fn a_sidecar_that_cannot_be_read_rolls_back_the_batch_written_before_it() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("first.xmp");
        let second = dir.path().join("second.xmp");
        std::fs::write(&first, SEED).expect("seed the sidecar the first write overwrites");
        std::fs::write(&second, SEED).expect("seed the sidecar that becomes unreadable");
        let items = two_items(dir.path());
        let mut stored: Vec<String> = Vec::new();
        let mut writes = 0;

        // Write-only: present, replaceable, unreadable — a sidecar the request
        // has to read before it may overwrite it, and cannot.
        std::fs::set_permissions(&second, std::fs::Permissions::from_mode(0o200))
            .expect("make the second sidecar unreadable");
        let outcome = commit_with(
            &items,
            &mut |id: &ArrayString<64>, _data: &AbstractData| {
                stored.push(id.to_string());
                Ok(())
            },
            &mut |_data: &AbstractData| {
                writes += 1;
                std::fs::write(&first, WRITTEN).map_err(io::Error::other)
            },
        );
        std::fs::set_permissions(&second, std::fs::Permissions::from_mode(0o600))
            .expect("make the second sidecar readable again");

        let err = outcome.expect_err("an unreadable sidecar must fail the request");
        assert_eq!(err.kind, crate::error::ErrorKind::IO);
        assert_eq!(
            writes, 1,
            "the first item's write did land before the failure"
        );
        assert!(
            stored.is_empty(),
            "the store phase must not run after a failed capture: {stored:?}"
        );
        assert_eq!(
            std::fs::read(&first).expect("the sidecar is still there"),
            SEED,
            "the sidecar written before the failed capture must be put back"
        );
    }

    /// A sidecar that exists but cannot be *read* fails the request before
    /// anything is written, rather than being written blind: the previous bytes
    /// are the only record of what was in the file, so a request that cannot
    /// read them has no way to keep the promise the rollback makes.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_sidecar_fails_the_request_without_writing_anything() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let sidecar = dir.path().join("first.xmp");
        std::fs::write(&sidecar, SEED).expect("seed the sidecar");
        let items = vec![item("one", photo(dir.path(), "first.jpg", &["alpha"]))];
        let mut stored: Vec<String> = Vec::new();
        let mut writes = 0;

        // Write-only: replaceable, unreadable.
        std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o200))
            .expect("make the sidecar unreadable");
        let outcome = commit_with(
            &items,
            &mut |id: &ArrayString<64>, _data: &AbstractData| {
                stored.push(id.to_string());
                Ok(())
            },
            &mut |_data: &AbstractData| {
                writes += 1;
                Ok(())
            },
        );
        std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o600))
            .expect("make the sidecar readable again");

        let err = outcome.expect_err("an unreadable sidecar must fail the request");
        assert_eq!(err.kind, crate::error::ErrorKind::IO);
        assert_eq!(writes, 0, "the write must not be attempted at all");
        assert!(stored.is_empty(), "and nothing is stored: {stored:?}");
        assert_eq!(
            std::fs::read(&sidecar).expect("still readable"),
            SEED,
            "the sidecar is untouched"
        );
    }
}
