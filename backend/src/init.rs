use crate::model::config::AppConfig;
use crate::storage::files::get_data_path;
use env_logger::{Builder, WriteStyle};
use log::kv::Key;
use std::io::Write;

pub fn initialize_logger() {
    Builder::new()
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
        .filter(None, log::LevelFilter::Info)
        .filter(Some("rocket"), log::LevelFilter::Warn)
        .init();
}

use log::{error, info};
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

use crate::model::config::APP_CONFIG;

pub fn initialize_folder() {
    let (data_home, image_home, upload_folder) = {
        let config = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned");
        let data_home = config
            .data_home
            .clone()
            .unwrap_or_else(|| get_data_path().clone());
        let image_home = config.image_home.clone();
        let upload_folder = config.upload_folder.clone();
        (data_home, image_home, upload_folder)
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
}

use crate::storage::db::DATA_TABLE;
use crate::storage::db::TREE;
use std::fs;

pub fn initialize_file() {
    let root = get_data_path();

    // Ensure DATA_TABLE exists so that read-only callers (e.g. init_dir_album_cache)
    // never see TableDoesNotExist on a fresh or reset database.
    {
        let txn = TREE
            .in_disk
            .begin_write()
            .expect("failed to begin write transaction");
        txn.open_table(DATA_TABLE).expect("failed to open table");
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
pub fn initialize() {
    // Config must be initialized first to ensure 'config.toml' exists for subsequent subsystems.
    AppConfig::init();

    // Ensure storage folders exist before trying to download FFmpeg into them
    initialize_folder();

    check_ffmpeg_and_ffprobe();
    initialize_file();
}
