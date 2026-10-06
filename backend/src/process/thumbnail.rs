use crate::{model::abstract_data::AbstractData, process::misc::small_width_height};
use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, ImageFormat};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Sequence number for temporary thumbnails. Duplicate uploads index the same
/// content-addressed path concurrently, so two writers must never pick the
/// same temporary name.
static TEMP_FILE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Generate a JPEG thumbnail for an **image** asset, propagating
/// every error with clear human‑readable context strings.
pub fn generate_thumbnail_for_image(
    abstract_data: &mut AbstractData,
    dynamic_image: &DynamicImage,
) -> Result<()> {
    let (compressed_width, compressed_height) =
        small_width_height(abstract_data.width(), abstract_data.height(), 720);

    let thumbnail_image = dynamic_image
        .thumbnail_exact(compressed_width, compressed_height)
        .to_rgb8();

    // Resolve parent directory of the compressed path
    let compressed_path = abstract_data.compressed_path();
    let parent_path = compressed_path.parent().ok_or_else(|| {
        anyhow!(
            "failed to determine parent directory of {}",
            compressed_path.display()
        )
    })?;

    // Ensure the directory exists
    std::fs::create_dir_all(parent_path).context(format!(
        "failed to create directory tree {}",
        parent_path.display()
    ))?;

    // Persist the thumbnail as JPEG
    write_atomically(&compressed_path, |temp_path| {
        thumbnail_image.save_with_format(temp_path, ImageFormat::Jpeg)
    })
    .with_context(|| {
        format!(
            "failed to save JPEG thumbnail to {}",
            compressed_path.display()
        )
    })?;

    Ok(())
}

/// Write `dest` through a temporary sibling and rename it into place.
///
/// `image`'s savers create (and truncate) their destination before they start
/// encoding and flush the bytes at the end, so writing `dest` directly exposes
/// an empty or partly written file for as long as the encode runs. The
/// destination is content-addressed and gets regenerated while the server is
/// serving it: the filesystem watcher re-indexes an upload a second or two
/// after the upload handler already indexed it, and rotating an image
/// regenerates its thumbnail. A concurrent `GET /object/compressed/...` would
/// then read the half-written file and answer HTTP 200 with a body that is not
/// a complete JPEG — the browser cannot decode it, and the frontend never
/// retries, so the tile stays blank.
///
/// Renaming within one directory is atomic, so readers only ever observe a
/// complete file: either the previous one or the new one.
///
/// Failures remove the temporary file so a failed write does not leave
/// partials behind next to the thumbnails.
fn write_atomically(
    dest: &Path,
    write: impl FnOnce(&Path) -> image::ImageResult<()>,
) -> Result<()> {
    let file_name = dest.file_name().map_or_else(
        || String::from("thumbnail"),
        |name| name.to_string_lossy().into_owned(),
    );
    let seq = TEMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp_path = dest.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), seq));

    if let Err(err) = write(&temp_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err.into());
    }

    if let Err(err) = std::fs::rename(&temp_path, dest) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err).with_context(|| {
            format!(
                "failed to move JPEG thumbnail into place at {}",
                dest.display()
            )
        });
    }

    Ok(())
}
