use crate::constant::SHOULD_SWAP_WIDTH_HEIGHT_ROTATION;
use crate::model::abstract_data::AbstractData;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use image::DynamicImage;

pub fn fix_image_orientation(abstract_data: &AbstractData, dynamic_image: &mut DynamicImage) {
    if let Some(exif_vec) = abstract_data.exif_vec()
        && let Some(orientation) = exif_vec.get("Orientation")
    {
        match orientation.as_str() {
            "row 0 at right and column 0 at top" => {
                *dynamic_image = dynamic_image.rotate90();
            }
            "row 0 at bottom and column 0 at right" => {
                *dynamic_image = dynamic_image.rotate180();
            }
            "row 0 at left and column 0 at bottom" => {
                *dynamic_image = dynamic_image.rotate270();
            }
            _ => (),
        }
    }
}

pub fn fix_image_width_height(abstract_data: &mut AbstractData) {
    if let Some(exif_vec) = abstract_data.exif_vec()
        && let Some(orientation) = exif_vec.get("Orientation")
    {
        match orientation.as_str() {
            "row 0 at right and column 0 at top" | "row 0 at left and column 0 at bottom" => {
                abstract_data.swap_width_height();
            }
            _ => (),
        }
    }
}

pub fn fix_video_width_height(abstract_data: &mut AbstractData) {
    let should_swap = if let Some(exif_vec) = abstract_data.exif_vec() {
        if let Some(rotation) = exif_vec.get("rotation") {
            SHOULD_SWAP_WIDTH_HEIGHT_ROTATION.contains(&rotation.trim())
        } else {
            false
        }
    } else {
        false
    };
    if should_swap {
        abstract_data.swap_width_height();
    }
}

use std::fs::read;
use std::path::PathBuf;

/// Generate a `DynamicImage` either from the original image or
/// from its thumbnail, adding *context* at every fallible step.
pub fn generate_dynamic_image(abstract_data: &AbstractData) -> Result<DynamicImage> {
    let img_path = if abstract_data.is_image() {
        abstract_data.source_path()
    } else {
        PathBuf::from(abstract_data.thumbnail_path())
    };

    let dynamic_image = decode_image(&img_path)
        .context(format!("failed to decode image: {}", img_path.display()))?;

    Ok(dynamic_image)
}

fn decode_image(file_path: &PathBuf) -> Result<DynamicImage> {
    let file_in_memory = read(file_path).context(format!(
        "failed to read file into memory: {}",
        file_path.display()
    ))?;

    let decoders = vec![image_crate_decoder];

    for decoder in decoders {
        if let Ok(decoded_image) = decoder(&file_in_memory) {
            return Ok(decoded_image);
        }
    }

    bail!("all decoders failed for file: {}", file_path.display());
}

fn image_crate_decoder(file_in_memory: &[u8]) -> Result<DynamicImage> {
    let dynamic_image = image::load_from_memory(file_in_memory)
        .context("image crate failed to decode image from memory")?;
    Ok(dynamic_image)
}

use std::process::Command;
/// Creates a base `ffmpeg` command with flags to ensure it runs silently.
/// This prevents duplicating arguments and ensures all ffmpeg calls are quiet.
pub fn create_silent_ffmpeg_command() -> Command {
    let mut cmd = Command::new("ffmpeg");
    // These global options must come before the input/output options.
    cmd.args(["-v", "quiet", "-hide_banner", "-nostats", "-nostdin"]);
    cmd
}

use image_hasher::HasherConfig;
pub fn generate_thumbhash(dynamic_image_rotated: &DynamicImage) -> Vec<u8> {
    let resized_image = dynamic_image_rotated.thumbnail_exact(100, 100);
    let rgba_image = resized_image.to_rgba8();
    let (swidth, sheight) = (rgba_image.width(), rgba_image.height());

    thumbhash::rgba_to_thumb_hash(swidth as usize, sheight as usize, &rgba_image)
}

pub fn generate_phash(dynamic_image_rotated: &DynamicImage) -> Vec<u8> {
    let hasher = HasherConfig::new().to_hasher();
    let phash = hasher.hash_image(dynamic_image_rotated);
    phash.as_bytes().to_vec()
}

/// Return `(width, height)` for an already‑decoded **image**.
/// Pure function ‑ no fallible operations.
pub fn generate_image_width_height(dynamic_image: &DynamicImage) -> (u32, u32) {
    (dynamic_image.width(), dynamic_image.height())
}

/// Resize dimensions so that the smaller side equals `target_short_side`, preserving aspect ratio.
///
/// This function ensures that the shortest side of the image is scaled down to `target_short_side`
/// if it exceeds that value. If the shortest side is already smaller than or equal to
/// `target_short_side`, the dimensions remain unchanged.
///
/// # Parameters
/// - `width`: original width of the image.
/// - `height`: original height of the image.
/// - `target_short_side`: the maximum allowed size for the smaller side of the image.
///
/// # Returns
/// A tuple `(new_width, new_height)` representing the scaled dimensions.
pub fn small_width_height(width: u32, height: u32, target_short_side: u32) -> (u32, u32) {
    // Identify the length of the smaller side of the original image
    let min_dimension = std::cmp::min(width, height);

    // Only scale if the smaller side is larger than the target limit
    if min_dimension > target_short_side {
        if width < height {
            // Width is the smaller side (Portrait or Landscape where width < height isn't standard, but logically valid)
            // Scale width to target, calculate height proportionally
            // Formula: new_height = original_height * (target / original_width)
            (target_short_side, height * target_short_side / width)
        } else {
            // Height is the smaller side (Landscape or Square)
            // Scale height to target, calculate width proportionally
            // Formula: new_width = original_width * (target / original_height)
            (width * target_short_side / height, target_short_side)
        }
    } else {
        // The image's smaller side is within the limit, return original dimensions
        (width, height)
    }
}
