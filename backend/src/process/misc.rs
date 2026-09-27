use crate::constant::SHOULD_SWAP_WIDTH_HEIGHT_ROTATION;
use crate::model::abstract_data::AbstractData;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use image::DynamicImage;

/// The three pure rotations `ExifTool` reports in `Orientation`.
///
/// `ExifTool` prints the value from the `%orientation` table in
/// `Image/ExifTool/Exif.pm`, measured with exiftool 13.59 (the version
/// `just install-exiftool` pins; the distro package CI and the Docker image
/// install prints the same strings). The two quarter turns also transpose the
/// stored width and height, which is [`fix_image_width_height`]'s half of the
/// job; a half turn does not.
///
/// ```text
/// 1  Horizontal (normal)                                  no rotation
/// 2  Mirror horizontal                                    no rotation
/// 3  Rotate 180                                           rotate 180
/// 4  Mirror vertical                                      no rotation
/// 5  Mirror horizontal and rotate 270 CW                  no rotation
/// 6  Rotate 90 CW                                         rotate 90, swap w/h
/// 7  Mirror horizontal and rotate 90 CW                   no rotation
/// 8  Rotate 270 CW                                        rotate 270, swap w/h
/// ```
///
/// The mirrored variants are deliberately left alone: a mirror is not a
/// rotation, and matching them loosely (on a `rotate 270` substring, say) would
/// start transposing images that were never transposed. The names are matched
/// exactly, so a future `ExifTool` that rewords them degrades to "no rotation"
/// rather than to a wrong one — `exif_rotation_is_matched_exactly` pins that
/// consequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rotation {
    Clockwise90,
    Half,
    Clockwise270,
}

/// The rotation an `exifVec` `Orientation` value asks for, if any.
fn exif_rotation(abstract_data: &AbstractData) -> Option<Rotation> {
    let orientation = abstract_data.exif_vec()?.get("Orientation")?;
    match orientation.as_str() {
        "Rotate 90 CW" => Some(Rotation::Clockwise90),
        "Rotate 180" => Some(Rotation::Half),
        "Rotate 270 CW" => Some(Rotation::Clockwise270),
        _ => None,
    }
}

pub fn fix_image_orientation(abstract_data: &AbstractData, dynamic_image: &mut DynamicImage) {
    match exif_rotation(abstract_data) {
        Some(Rotation::Clockwise90) => {
            *dynamic_image = dynamic_image.rotate90();
        }
        Some(Rotation::Half) => {
            *dynamic_image = dynamic_image.rotate180();
        }
        Some(Rotation::Clockwise270) => {
            *dynamic_image = dynamic_image.rotate270();
        }
        None => (),
    }
}

pub fn fix_image_width_height(abstract_data: &mut AbstractData) {
    let transposes = matches!(
        exif_rotation(abstract_data),
        Some(Rotation::Clockwise90 | Rotation::Clockwise270)
    );
    if transposes {
        abstract_data.swap_width_height();
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

#[cfg(test)]
mod tests {
    use super::{fix_image_orientation, fix_image_width_height};
    use crate::model::abstract_data::AbstractData;
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use arrayvec::ArrayString;
    use image::{DynamicImage, Rgb, RgbImage};
    use std::collections::BTreeMap;

    /// A 2x3 image record whose `exifVec` carries `orientation`, decoded pixels
    /// to hand to the rotation fix, and the width/height the decoder reported.
    ///
    /// The dimensions are deliberately *not* square, so a rotation is visible in
    /// the image shape as well as in the record.
    fn landscape_with(orientation: &str) -> (AbstractData, DynamicImage) {
        let mut metadata = ImageMetadata::new(0, 2, 3, "jpg".to_string());
        metadata
            .exif_vec
            .insert("Orientation".to_string(), orientation.to_string());
        let record = AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(
                ArrayString::from("orientation").expect("hash fits"),
                ObjectType::Image,
            ),
            metadata,
        });
        (
            record,
            DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 3, Rgb([1, 2, 3]))),
        )
    }

    /// The whole EXIF `Orientation` table as ExifTool prints it, with the
    /// rotation and the width/height transpose each value must produce. Every
    /// one of the eight values is here on purpose: the mirrored ones are the
    /// regression this table rules out, because a matcher loose enough to catch
    /// `Mirror horizontal and rotate 270 CW` as a rotation would also rotate
    /// images that must not move.
    #[test]
    fn only_the_pure_exiftool_rotations_are_applied() {
        // (exiftool value, rotated image shape, record shape after the fix)
        let cases = [
            ("Horizontal (normal)", (2, 3), (2, 3)),
            ("Mirror horizontal", (2, 3), (2, 3)),
            // A half turn keeps both shapes; only a quarter turn transposes.
            ("Rotate 180", (2, 3), (2, 3)),
            ("Mirror vertical", (2, 3), (2, 3)),
            ("Mirror horizontal and rotate 270 CW", (2, 3), (2, 3)),
            ("Rotate 90 CW", (3, 2), (3, 2)),
            ("Mirror horizontal and rotate 90 CW", (2, 3), (2, 3)),
            ("Rotate 270 CW", (3, 2), (3, 2)),
        ];

        for (orientation, rotated, stored) in cases {
            let (mut record, mut image) = landscape_with(orientation);
            fix_image_orientation(&record, &mut image);
            fix_image_width_height(&mut record);

            assert_eq!(
                (image.width(), image.height()),
                (rotated.0, rotated.1),
                "{orientation:?}: the decoded image must be rotated accordingly"
            );
            assert_eq!(
                (record.width(), record.height()),
                (stored.0, stored.1),
                "{orientation:?}: width/height must only be transposed for a \
                 quarter-turn, because the rotation above already swapped them"
            );
        }
    }

    /// An `Orientation` the engine did not print is not a rotation, and the
    /// record is left as the decoder reported it. This is the deliberate
    /// consequence of matching ExifTool's names exactly: an unrecognised value
    /// (a reworded name in a future release, a value left behind by the previous
    /// in-process reader, a tag carrying a number instead of a name) yields an
    /// untouched image rather than a wrongly turned one.
    #[test]
    fn exif_rotation_is_matched_exactly() {
        for orientation in [
            // The value the retired in-process reader wrote: a stale index built
            // by an older release still carries it until it is reindexed.
            "row 0 at right and column 0 at top",
            // A raw EXIF orientation number, which is what `-n` would print.
            "6",
            // A near miss on the wording, which must not be treated as a turn.
            "Rotate 90",
            "rotate 90 CW",
            "Auto-rotate",
        ] {
            let (mut record, mut image) = landscape_with(orientation);
            fix_image_orientation(&record, &mut image);
            fix_image_width_height(&mut record);

            assert_eq!(
                (image.width(), image.height()),
                (2, 3),
                "{orientation:?} is not a rotation this engine applies"
            );
            assert_eq!(
                (record.width(), record.height()),
                (2, 3),
                "{orientation:?} must not transpose the stored dimensions"
            );
        }
    }

    /// An image with no `Orientation` at all is the common case, and it has to
    /// survive both fixes untouched. The negative control is the same code path
    /// with a rotation, so an empty map cannot make this pass for the wrong
    /// reason.
    #[test]
    fn an_image_without_an_orientation_is_left_alone() {
        let mut record = AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(
                ArrayString::from("no-orientation").expect("hash fits"),
                ObjectType::Image,
            ),
            metadata: ImageMetadata::new(0, 2, 3, "jpg".to_string()),
        });
        let mut image = DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 3, Rgb([1, 2, 3])));
        assert!(record.exif_vec().is_some_and(BTreeMap::is_empty));

        fix_image_orientation(&record, &mut image);
        fix_image_width_height(&mut record);

        assert_eq!((image.width(), image.height()), (2, 3));
        assert_eq!((record.width(), record.height()), (2, 3));
    }
}
