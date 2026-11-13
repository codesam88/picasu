use crate::model::abstract_data::AbstractData;
use log::warn;
use std::path::Path;

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

/// Remove the asset path `target` from `data`, deleting its file + sidecar
/// from disk. Returns `true` if the record still holds its path (and should
/// be persisted), `false` once the path is gone. The serial writer owns the
/// thumbnail and DB removal, so this never touches content-addressed
/// renditions. Albums (no path) return `false`.
#[quint_annotations::operation("OP-12")]
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

    path_slot.is_some()
}
