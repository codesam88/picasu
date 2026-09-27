use crate::process::thumbnail::generate_thumbnail_for_image;
use anyhow::{Context, Result};

use crate::model::abstract_data::AbstractData;
use crate::process::exif::{exif_map_from_record, generate_exif_for_video, read_metadata_record};
use crate::process::misc::{
    fix_image_orientation, fix_image_width_height, fix_video_width_height, generate_dynamic_image,
    generate_image_width_height, generate_phash, generate_thumbhash,
};
use crate::process::xmp::native_metadata_for;

/// Analyse the newly‑imported **image** and populate the `AbstractData` record.
pub fn process_image_info(abstract_data: &mut AbstractData) -> Result<()> {
    // One `ExifTool` read serves both metadata consumers: the EXIF map and the
    // native fields are two projections of the same record, so a second read
    // would only double the indexing cost.
    let record = read_metadata_record(&abstract_data.source_path());
    if let Some(exif_vec) = abstract_data.exif_vec_mut() {
        *exif_vec = record
            .as_ref()
            .map(exif_map_from_record)
            .unwrap_or_default();
    }

    // Native fields: XMP from the sidecar when one exists, IPTC and PNG text
    // from the image either way.
    let native = native_metadata_for(&abstract_data.source_path(), record.as_ref().ok());
    abstract_data.tag_mut().extend(native.tags);
    if abstract_data.description().is_none() {
        abstract_data.set_description(native.description);
    }
    if abstract_data.rating().is_none() {
        abstract_data.set_rating(native.rating);
    }

    // Decode image to DynamicImage
    let mut dynamic_image = generate_dynamic_image(abstract_data)
        .context("failed to decode image into DynamicImage")?;

    // Measure & possibly fix width/height
    let (width, height) = generate_image_width_height(&dynamic_image);
    abstract_data.set_width(width);
    abstract_data.set_height(height);
    fix_image_width_height(abstract_data);

    // Adjust orientation if required
    fix_image_orientation(abstract_data, &mut dynamic_image);

    // Compute perceptual hashes
    abstract_data.set_thumbhash(generate_thumbhash(&dynamic_image));
    abstract_data.set_phash(generate_phash(&dynamic_image));

    // Generate on‑disk JPEG thumbnail
    generate_thumbnail_for_image(abstract_data, &dynamic_image)
        .context("failed to generate JPEG thumbnail for image")?;

    Ok(())
}

/// Analyse the newly‑imported **video** and populate the `AbstractData` record.
pub fn process_video_info(abstract_data: &mut AbstractData) -> Result<()> {
    // Extract EXIF‑like metadata via ffprobe
    let exif = generate_exif_for_video(abstract_data)
        .context("failed to extract video metadata via ffprobe")?;
    if let Some(exif_vec) = abstract_data.exif_vec_mut() {
        *exif_vec = exif;
    }

    // Native fields: XMP from the sidecar when one exists, the packet in the
    // file otherwise. A separate `ExifTool` read from the ffprobe one above —
    // a video carries neither an IIM record nor PNG text, so the mapping sees
    // the XMP family alone.
    let record = read_metadata_record(&abstract_data.source_path());
    let native = native_metadata_for(&abstract_data.source_path(), record.as_ref().ok());
    abstract_data.tag_mut().extend(native.tags);
    if abstract_data.description().is_none() {
        abstract_data.set_description(native.description);
    }
    if abstract_data.rating().is_none() {
        abstract_data.set_rating(native.rating);
    }

    // Get logical dimensions and fix if rotated
    let (width, height) = crate::process::video::generate_video_width_height(abstract_data)
        .context("failed to obtain video width/height")?;
    abstract_data.set_width(width);
    abstract_data.set_height(height);
    fix_video_width_height(abstract_data);

    // Produce thumbnail from first frame
    crate::process::video::generate_thumbnail_for_video(abstract_data)
        .context("failed to generate video thumbnail via ffmpeg")?;

    // Decode the first frame for hashing purposes
    let dynamic_image = generate_dynamic_image(abstract_data)
        .context("failed to decode first video frame into DynamicImage")?;

    // Compute perceptual hashes
    abstract_data.set_thumbhash(generate_thumbhash(&dynamic_image));
    abstract_data.set_phash(generate_phash(&dynamic_image));

    Ok(())
}
