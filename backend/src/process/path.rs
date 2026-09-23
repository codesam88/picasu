use crate::model::abstract_data::AbstractData;
use crate::storage::asset_store;
use log::{info, warn};
use std::path::{Path, PathBuf};

/// Remove the asset's file and its `.xmp` sidecar from disk.
fn remove_asset_file(file: &str) {
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

/// Normalize a stored asset path to an absolute path, resolving relative
/// paths against the configured image home.
pub fn normalize_asset_path(file: &str) -> PathBuf {
    let p = Path::new(file);
    if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(image_home) = crate::storage::files::get_resolved_image_home() {
        image_home.join(p)
    } else {
        p.to_path_buf()
    }
}

/// Remove the asset path `target` from `data`, deleting its file + sidecar
/// from disk.  Returns `true` if the record still holds its canonical path
/// (and should be persisted), `false` once the path is gone (thumbnail + DB
/// removal caller's responsibility). Albums (no path) return `false`.
pub fn prune_asset_path(data: &mut AbstractData, target: &Path) -> bool {
    remove_asset_file(target.to_string_lossy().as_ref());

    let Some(path_slot) = data.path_mut() else {
        return false;
    };

    if path_slot
        .as_ref()
        .is_some_and(|a| Path::new(&a.file) == target)
    {
        *path_slot = None;
    }

    if path_slot.is_none() {
        remove_compressed_thumbnail(data);
        false
    } else {
        true
    }
}

/// Remove the record's path if its file no longer exists on disk.  Returns
/// `true` if the record still holds its canonical path, `false` once the path
/// is gone (thumbnail already removed by this call). Albums return `false`.
pub fn prune_stale_asset_path(data: &mut AbstractData) -> bool {
    let Some(path_slot) = data.path_mut() else {
        return false;
    };

    let stale = path_slot
        .as_ref()
        .is_some_and(|a| !normalize_asset_path(&a.file).exists());
    if stale {
        *path_slot = None;
    }

    if path_slot.is_none() {
        remove_compressed_thumbnail(data);
        false
    } else {
        true
    }
}

fn remove_compressed_thumbnail(data: &AbstractData) {
    let content_hash = data.hash();

    // Check if other assets still share this content hash.
    // DUPE_INDEX still contains the current asset (removed later by
    // FlushTreeTask), so len > 1 means another asset references the hash.
    let other_refs = match asset_store::get_dupe_ids(&content_hash) {
        Ok(ids) => ids.len(),
        Err(_) => 0,
    };
    if other_refs > 1 {
        info!(
            "Preserving thumbnail for hash {content_hash}: {other_refs} assets still reference it"
        );
        return;
    }

    let thumb = data.compressed_path();
    if thumb.exists()
        && let Err(e) = std::fs::remove_file(&thumb)
    {
        warn!("Failed to delete thumbnail {}: {e}", thumb.display());
    }
}
