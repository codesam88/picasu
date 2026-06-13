use std::collections::HashSet;
use std::path::PathBuf;

use crate::public::structure::config::APP_CONFIG;

/// Resolve relative sync paths from config to absolute paths.
/// If `UROCISSA_PATH` is set, it's used as the base; otherwise the current working directory.
pub fn resolve_sync_paths(paths: HashSet<PathBuf>) -> HashSet<PathBuf> {
    let (base_path, append_subdir) = match std::env::var("UROCISSA_PATH") {
        Ok(p) => (PathBuf::from(p), true),
        Err(_) => (std::env::current_dir().unwrap_or_default(), false),
    };

    paths
        .into_iter()
        .map(|p| {
            if p.is_relative() {
                let mut resolved = base_path.clone();
                if append_subdir {
                    resolved.push("gallery-backend");
                }
                resolved.push(&p);
                resolved.canonicalize().unwrap_or(resolved)
            } else {
                p
            }
        })
        .collect()
}

/// Get the resolved sync paths from the current config.
pub fn get_resolved_sync_paths() -> HashSet<PathBuf> {
    let raw = APP_CONFIG
        .get()
        .unwrap()
        .read()
        .unwrap()
        .public
        .sync_paths
        .clone();
    resolve_sync_paths(raw)
}
