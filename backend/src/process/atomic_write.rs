use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Module-level sequence counter for temp file names. Starts at 0 and is
/// incremented with `Relaxed` ordering; uniqueness across a process is not
/// required, only monotonic progress to reduce collisions on pid reuse.
static SEQ: AtomicU64 = AtomicU64::new(0);

/// Build an obvious, owner-tagged temp file name for staging beside `target`.
///
/// Format: `.{event}-{filename}.{pid}.{seq}.tmp`. The pid/seq pair makes crash
/// residue identifiable and lets the startup sweep decide ownership; the
/// leading dot keeps the file hidden from casual directory listings.
pub(crate) fn temp_file_name(event: &str, filename: &str) -> String {
    let pid = std::process::id();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!(".{event}-{filename}.{pid}.{seq}.tmp")
}

/// Write `target` through a temp file beside it, then persist (rename).
///
/// `write` is handed the temp path and is responsible for putting the desired
/// bytes there. The temp file is staged with a `NamedTempFile` so it is
/// removed on every error path (failed write, failed persist, or panic drop).
/// On success the temp is atomically persisted over `target`.
///
/// The temp name embeds the owning pid and a sequence number so crash residue
/// is obvious and sweepable; if a chosen name already exists (pid reuse plus
/// stale residue) we advance the sequence and retry a small bounded number
/// of times, returning the last error if all attempts fail.
pub(crate) fn write_via_temp<F>(target: &Path, event: &str, write: F) -> io::Result<()>
where
    F: FnOnce(&Path) -> io::Result<()>,
{
    let parent = target.parent().unwrap_or(Path::new("."));
    let filename = target.file_name().and_then(|n| n.to_str()).unwrap_or("xmp");

    let mut last_err: Option<io::Error> = None;
    for _ in 0..5 {
        let tmp = parent.join(temp_file_name(event, filename));

        let file = match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => f,
            Err(e) => {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    last_err = Some(e);
                    continue;
                }
                return Err(e);
            }
        };

        let temp_path = match tempfile::TempPath::try_from_path(&tmp) {
            Ok(path) => path,
            Err(err) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(err);
            }
        };
        let staged = tempfile::NamedTempFile::from_parts(file, temp_path);

        write(staged.path())?;

        staged.persist(target).map_err(|e| e.error)?;
        return Ok(());
    }

    Err(last_err.unwrap_or_else(|| io::Error::other("failed to create temp file")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn temp_file_name_format() {
        let name = temp_file_name("sidecar", "photo.xmp");
        let middle = name
            .strip_prefix(".sidecar-photo.xmp.")
            .expect("prefix")
            .strip_suffix(".tmp")
            .expect("suffix");
        let parts: Vec<&str> = middle.split('.').collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].chars().all(|c| c.is_ascii_digit()));
        assert!(parts[1].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn a_failed_temp_write_leaves_the_existing_target_unchanged() {
        let dir = tempdir().expect("temp dir");
        let target = dir.path().join("target.xmp");
        std::fs::write(&target, b"old bytes").expect("seed");

        let result = write_via_temp(&target, "sidecar", |t| {
            std::fs::write(t, b"partial").map_err(std::io::Error::other)?;
            Err(std::io::Error::other("injected failure"))
        });

        assert!(result.is_err());
        assert_eq!(
            std::fs::read(&target).expect("target readable"),
            b"old bytes",
            "existing target bytes must be untouched"
        );
        let entries: Vec<_> = std::fs::read_dir(dir.path()).expect("read dir").collect();
        assert_eq!(entries.len(), 1, "no temp file must remain");
    }

    #[test]
    fn a_failed_temp_write_leaves_an_absent_target_absent() {
        let dir = tempdir().expect("temp dir");
        let target = dir.path().join("target.xmp");

        let result = write_via_temp(&target, "sidecar", |t| {
            std::fs::write(t, b"partial").map_err(std::io::Error::other)?;
            Err(std::io::Error::other("injected failure"))
        });

        assert!(result.is_err());
        assert!(!target.exists(), "absent target must stay absent");
        let entries: Vec<_> = std::fs::read_dir(dir.path()).expect("read dir").collect();
        assert!(entries.is_empty(), "no temp file must remain");
    }

    #[test]
    fn a_successful_write_replaces_the_target() {
        let dir = tempdir().expect("temp dir");
        let target = dir.path().join("target.xmp");
        std::fs::write(&target, b"old bytes").expect("seed");

        let result = write_via_temp(&target, "sidecar", |t| {
            std::fs::write(t, b"new bytes").map_err(std::io::Error::other)
        });

        assert!(result.is_ok());
        assert_eq!(
            std::fs::read(&target).expect("target readable"),
            b"new bytes",
            "target must hold the new bytes"
        );
    }
}
