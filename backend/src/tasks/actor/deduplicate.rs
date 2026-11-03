use crate::model::abstract_data::AbstractData;
use anyhow::Result;
use arrayvec::ArrayString;
use std::path::Path;

pub(crate) fn deduplicate_task(
    path: &Path,
    hash: ArrayString<64>,
    presigned_album_id: Option<ArrayString<64>>,
) -> Result<Option<AbstractData>> {
    let mut abstract_data = AbstractData::new(path, hash)?;

    if let Some(album_id) = presigned_album_id {
        abstract_data.set_album(Some(album_id));
    } else if let Some(parent) = path.parent() {
        // For filesystem-indexed photos, set the album to the parent
        // directory's dir-album so album-contents page filters match.
        if let Ok(album_id) =
            crate::process::dir_album::get_or_create_dir_album(parent.to_path_buf())
        {
            abstract_data.set_album(Some(album_id));
        }
    }

    // Path-primary model: each physical file gets its own record.
    // Same-hash files are tracked via DUPE_INDEX, never merged into one record.
    // Always return Some so the index stage processes this file independently.
    Ok(Some(abstract_data))
}
