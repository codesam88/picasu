use crate::model::abstract_data::AbstractData;
use log::warn;
use std::path::Path;

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
        let thumb = data.compressed_path();
        if thumb.exists()
            && let Err(e) = std::fs::remove_file(&thumb)
        {
            warn!("Failed to delete thumbnail {}: {e}", thumb.display());
        }
        false
    } else {
        true
    }
}
