use crate::{model::abstract_data::AbstractData, process::misc::small_width_height};
use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, ImageFormat};

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
    let binding = abstract_data.compressed_path();
    let parent_path = binding.parent().ok_or_else(|| {
        anyhow!(
            "failed to determine parent directory of {}",
            abstract_data.compressed_path().display()
        )
    })?;

    // Ensure the directory exists
    std::fs::create_dir_all(parent_path).context(format!(
        "failed to create directory tree {}",
        parent_path.display()
    ))?;

    // Persist the thumbnail as JPEG
    thumbnail_image
        .save_with_format(abstract_data.compressed_path(), ImageFormat::Jpeg)
        .context(format!(
            "failed to save JPEG thumbnail to {}",
            abstract_data.compressed_path().display()
        ))?;

    Ok(())
}
