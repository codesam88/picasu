use crate::model::abstract_data::AbstractData;
use log::warn;
use std::path::{Path, PathBuf};

/// Remove the original file and its `.xmp` sidecar from disk.
fn remove_alias_file(file: &str) {
    let original = Path::new(file);
    if let Err(e) = std::fs::remove_file(original)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!("Failed to delete file {}: {e}", original.display());
    }
    let sidecar = original.with_extension("xmp");
    if sidecar.exists()
        && let Err(e) = std::fs::remove_file(&sidecar)
    {
        warn!("Failed to delete sidecar {}: {e}", sidecar.display());
    }
}

/// Normalize an alias file path to an absolute path, resolving relative paths
/// against the configured image home.
pub fn normalize_alias_path(file: &str) -> PathBuf {
    let p = Path::new(file);
    if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(image_home) = crate::storage::files::get_resolved_image_home() {
        image_home.join(p)
    } else {
        p.to_path_buf()
    }
}

/// Remove the given alias path from `data`, deleting its file + sidecar from
/// disk.  Returns `true` if the record still has aliases remaining (and should
/// be persisted), `false` if the record is now empty (thumbnail + DB removal
/// caller's responsibility).
pub fn prune_alias_paths(data: &mut AbstractData, target: &Path) -> bool {
    remove_alias_file(target.to_string_lossy().as_ref());

    let Some(alias_vec) = data.alias_mut() else {
        return false;
    };

    alias_vec.retain(|a| Path::new(&a.file) != target);

    if alias_vec.is_empty() {
        remove_compressed_thumbnail(data);
        false
    } else {
        true
    }
}

/// Remove aliases whose files no longer exist on disk.  Returns `true` if the
/// record still has aliases remaining, `false` if the record is now empty
/// (thumbnail already removed by this call).
pub fn prune_stale_aliases(data: &mut AbstractData) -> bool {
    let Some(alias_vec) = data.alias_mut() else {
        return false;
    };

    alias_vec.retain(|a| {
        let abs = normalize_alias_path(&a.file);
        abs.exists()
    });

    if alias_vec.is_empty() {
        remove_compressed_thumbnail(data);
        false
    } else {
        true
    }
}

fn remove_compressed_thumbnail(data: &AbstractData) {
    let thumb = data.compressed_path();
    if thumb.exists()
        && let Err(e) = std::fs::remove_file(&thumb)
    {
        warn!("Failed to delete thumbnail {}: {e}", thumb.display());
    }
}
