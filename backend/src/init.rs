use crate::model::config::AppConfig;
use crate::storage::files::get_data_path;
use env_logger::{Builder, Env, WriteStyle};
use log::kv::Key;
use std::io::Write;

/// Initializes the global logger.
///
/// The `RUST_LOG` environment variable is authoritative when set: its directives
/// apply as-is (e.g. `RUST_LOG=warn`, `RUST_LOG=rocket=debug,info`). When unset,
/// logging defaults to `info` globally and `warn` for the `rocket` target.
pub fn initialize_logger() {
    Builder::from_env(Env::default().default_filter_or("info,rocket=warn"))
        .write_style(WriteStyle::Auto)
        .format(|buf, record| {
            let ts = buf.timestamp();
            let tgt = record.target();

            let level_style = buf.default_level_style(record.level());
            let level = format!(
                "{}{}{}",
                level_style.render(),
                record.level(),
                level_style.render_reset()
            );

            let dur_raw = record
                .key_values()
                .get(Key::from("duration"))
                .map(|v| {
                    let s = format!("{v}");
                    if let Some(idx) = s.find(|c: char| c.is_alphabetic()) {
                        let (num, unit) = (&s[..idx], &s[idx..]);
                        if let Ok(val) = num.parse::<f32>() {
                            return format!("{val:.2} {unit}");
                        }
                    }
                    s
                })
                .unwrap_or_default();

            let dur = if dur_raw.is_empty() {
                " ".repeat(10)
            } else {
                format!("{dur_raw:>10}")
            };

            writeln!(buf, "{ts} {level} {tgt}")?;

            let message = format!("{}", record.args());
            let subsequent_indent = " ".repeat(11);
            let mut lines = message.lines();
            if let Some(first_line) = lines.next() {
                writeln!(buf, "{dur} {first_line}")?;
            }
            for line in lines {
                writeln!(buf, "{subsequent_indent}{line}")?;
            }
            Ok(())
        })
        .try_init()
        .ok();
}

use log::{error, info, warn};
use std::path::PathBuf;
use std::process::Command;

pub fn check_ffmpeg_and_ffprobe() {
    for command in &["ffmpeg", "ffprobe"] {
        match Command::new(command).arg("-version").output() {
            Ok(output) if output.status.success() => {
                let version_info = String::from_utf8_lossy(&output.stdout);
                let version_number = version_info
                    .lines()
                    .next()
                    .unwrap_or("Unknown version")
                    .split_whitespace()
                    .nth(2) // Get the third word
                    .unwrap_or("Unknown");
                info!("{command} version: {version_number}");
            }
            Ok(_) => {
                error!(
                    "`{command}` command was found, but it returned an error. Please ensure it's correctly installed."
                );
            }
            Err(_) => {
                error!(
                    "`{command}` is not installed or not available in PATH. Please install it before running the application."
                );
            }
        }
    }
}

pub fn initialize_folder() {
    let (data_home, image_home, users_home, upload_folder) = {
        let config = crate::model::config::with_config(crate::model::config::AppConfig::clone);
        let data_home = config
            .data_home
            .clone()
            .unwrap_or_else(|| get_data_path().clone());
        let image_home = config.image_home.clone();
        let users_home = config.users_home.clone();
        let upload_folder = config.upload_folder.clone();
        (data_home, image_home, users_home, upload_folder)
    };

    info!("Storage root initialized at: {}", data_home.display());
    std::fs::create_dir_all(data_home.join("db")).expect("failed to create db directory");
    std::fs::create_dir_all(data_home.join("object/compressed"))
        .expect("failed to create object/compressed directory");

    // Pre-create image root and uploads directory from config
    if let Some(ref root) = image_home {
        info!("Creating image root: {}", root.display());
        std::fs::create_dir_all(root).expect("failed to create image root directory");
        std::fs::create_dir_all(root.join(&upload_folder))
            .expect("failed to create upload folder directory");
    }

    // Sweep stale upload crash residue from configured roots only.
    let mut sweep_roots: Vec<PathBuf> = Vec::new();
    if let Some(root) = image_home {
        sweep_roots.push(root);
    }
    if let Some(root) = users_home {
        sweep_roots.push(root);
    }
    sweep_temp_files(&sweep_roots);
}

/// Parse an owned temp name into `(pid, seq)`. The callers all produce
/// `.{event}-{filename}.{pid}.{seq}.tmp` with `event` of `upload` or
/// `sidecar`; the filename may contain dots, so the two trailing numeric
/// components are parsed from the end.
fn parse_temp_name(name: &str) -> Option<(u32, u64)> {
    let stem = name.strip_suffix(".tmp")?;
    let rest = stem
        .strip_prefix(".upload-")
        .or_else(|| stem.strip_prefix(".sidecar-"))?;
    let last_dot = rest.rfind('.')?;
    let seq: u64 = rest[last_dot + 1..].parse().ok()?;
    let before_seq = &rest[..last_dot];
    let pid_dot = before_seq.rfind('.')?;
    let pid: u32 = before_seq[pid_dot + 1..].parse().ok()?;
    if before_seq[..pid_dot].is_empty() {
        return None;
    }
    Some((pid, seq))
}

/// Returns true when `pid` refers to a live process. `EPERM` counts as
/// alive; `ESRCH` as dead; pids above `i32::MAX` are dead.
#[allow(unsafe_code)]
fn pid_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid_i32) = i32::try_from(pid) else {
            return false;
        };
        // SAFETY: `kill(pid, 0)` performs permission/existence checks without
        // delivering a signal.
        let ret = unsafe { libc::kill(pid_i32, 0) };
        if ret == 0 {
            return true;
        }
        let err = std::io::Error::last_os_error();
        err.raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Walks `roots` and removes owned temp files whose owning pid is dead, or
/// whose owning pid is alive but whose mtime is older than one hour.
/// Returns the number of files successfully removed. A `warn!` is emitted
/// per failed removal and a single `info!` summarizes the count.
pub(crate) fn sweep_temp_files(roots: &[PathBuf]) -> usize {
    use std::time::{Duration, SystemTime};

    let mut removed = 0usize;
    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str() else {
                continue;
            };
            let Some((pid, _seq)) = parse_temp_name(name) else {
                continue;
            };
            let path = entry.path();
            let metadata = std::fs::metadata(path).ok();

            let mut remove = !pid_is_alive(pid);
            if !remove
                && let Some(md) = &metadata
                && let Ok(mtime) = md.modified()
                && let Ok(age) = SystemTime::now().duration_since(mtime)
                && age > Duration::from_secs(3600)
            {
                remove = true;
            }

            if !remove {
                continue;
            }

            match std::fs::remove_file(path) {
                Ok(()) => removed += 1,
                Err(err) => warn!("failed to remove temp file {}: {}", path.display(), err),
            }
        }
    }
    info!("swept {removed} stale temp file(s)");
    removed
}

use crate::storage::db::METADATA_TABLE;
use crate::storage::db::TREE;
use std::fs;

pub fn initialize_file() {
    let root = get_data_path();

    // Ensure the store-of-record tables exist so that read-only callers
    // (e.g. init_dir_album_cache, build_from_asset_tables) never see
    // TableDoesNotExist on a fresh or reset database.
    {
        let txn = TREE
            .in_disk
            .begin_write()
            .expect("failed to begin write transaction");
        txn.open_table(METADATA_TABLE)
            .expect("failed to open METADATA_TABLE");
        txn.open_table(crate::storage::db::ASSET_BY_PATH)
            .expect("failed to open ASSET_BY_PATH");
        txn.open_table(crate::storage::db::ASSET_BY_ID)
            .expect("failed to open ASSET_BY_ID");
        txn.open_table(crate::storage::db::DUPE_INDEX)
            .expect("failed to open DUPE_INDEX");
        txn.commit().expect("failed to commit transaction");
    }

    {
        let db_path = root.join("db/temp_db.redb");
        if fs::metadata(&db_path).is_ok() {
            match fs::remove_file(&db_path) {
                Ok(()) => {
                    info!("Clear tree cache");
                }
                Err(_) => {
                    error!("Fail to delete cache data {}", db_path.display());
                }
            }
        }
    }
    {
        let db_path = root.join("db/cache_db.redb");
        if fs::metadata(&db_path).is_ok() {
            match fs::remove_file(&db_path) {
                Ok(()) => {
                    info!("Clear query cache");
                }
                Err(_) => {
                    error!("Fail to delete cache data {}", db_path.display());
                }
            }
        }
    }
    {
        let db_path = root.join("db/expire_db.redb");
        if fs::metadata(&db_path).is_ok() {
            match fs::remove_file(&db_path) {
                Ok(()) => {
                    info!("Clear expire table");
                }
                Err(_) => {
                    error!("Fail to delete expire table {}", db_path.display());
                }
            }
        }
    }
}

/// Initializes all core application subsystems.
///
/// # Errors
/// Returns an error when the configuration is unusable (degenerate
/// login-throttle schedules); the caller must refuse to start.
pub fn initialize() -> anyhow::Result<()> {
    // Config must be initialized first to ensure 'config.toml' exists for subsequent subsystems.
    AppConfig::init()?;

    // Ensure storage folders exist before trying to download FFmpeg into them
    initialize_folder();

    check_ffmpeg_and_ffprobe();
    initialize_file();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::{FileTime, set_file_mtime};
    use std::fs;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    #[test]
    fn sweep_removes_dead_pid_temp() {
        let dir = TempDir::new().expect("create tempdir");
        let dead_pid = u32::MAX;
        let name = format!(".upload-residue.{dead_pid}.1.tmp");
        let residue = dir.path().join(&name);
        fs::write(&residue, b"x").expect("write residue");
        let n = sweep_temp_files(&[dir.path().to_path_buf()]);
        assert_eq!(n, 1);
        assert!(!residue.exists());
    }

    #[test]
    fn sweep_keeps_live_pid_fresh_temp() {
        let dir = TempDir::new().expect("create tempdir");
        let pid = std::process::id();
        let name = format!(".upload-live.{pid}.1.tmp");
        let kept = dir.path().join(&name);
        fs::write(&kept, b"x").expect("write");
        let n = sweep_temp_files(&[dir.path().to_path_buf()]);
        assert_eq!(n, 0);
        assert!(kept.exists());
    }

    #[test]
    fn sweep_removes_live_pid_old_temp() {
        let dir = TempDir::new().expect("create tempdir");
        let pid = std::process::id();
        let name = format!(".upload-live.{pid}.2.tmp");
        let old = dir.path().join(&name);
        fs::write(&old, b"x").expect("write");
        let past = SystemTime::now() - Duration::from_secs(2 * 3600);
        set_file_mtime(&old, FileTime::from_system_time(past)).expect("set mtime");
        let n = sweep_temp_files(&[dir.path().to_path_buf()]);
        assert_eq!(n, 1);
        assert!(!old.exists());
    }

    #[test]
    fn sweep_keeps_unrelated_and_malformed() {
        let dir = TempDir::new().expect("create tempdir");
        let notes = dir.path().join("notes.tmp");
        fs::write(&notes, b"y").expect("write");
        let malformed = dir.path().join(".upload-name.tmp");
        fs::write(&malformed, b"z").expect("write");
        let n = sweep_temp_files(&[dir.path().to_path_buf()]);
        assert_eq!(n, 0);
        assert!(notes.exists());
        assert!(malformed.exists());
    }
}
