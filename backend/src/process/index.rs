use crate::process::thumbnail::generate_thumbnail_for_image;
use anyhow::{Context, Result};

use crate::model::abstract_data::AbstractData;
use crate::process::exif::{exif_map_from_record, generate_exif_for_video, read_metadata_record};
use crate::process::misc::{
    fix_image_orientation, fix_image_width_height, fix_video_width_height, generate_dynamic_image,
    generate_image_width_height, generate_phash, generate_thumbhash,
};
use crate::process::xmp::{asset_metadata_for, native_metadata_for};

/// Run the metadata pipeline for one media asset, dispatching on its kind.
///
/// The single entry point for "derive this asset's metadata and its derived
/// data": the `ExifTool` read, sidecar precedence, the `furtherMetadata` bucket,
/// dimension fix-ups, perceptual hashes and the on-disk thumbnail. Both
/// orchestration modes go through here — the incremental indexer in
/// [`crate::tasks::actor::index::IndexTask`] and the filesystem rebuild — so
/// there is one implementation of that pipeline and no path where a rebuilt
/// asset is described by different rules than an indexed one.
///
/// A video's `pending` flag is deliberately *not* set here. That flag means "the
/// compressed form is not there yet", which is a property of the transcoding
/// step, not of the metadata read; the caller decides whether it is going to
/// transcode (see [`crate::workflow::index_media_file`]).
pub fn process_media_info(abstract_data: &mut AbstractData) -> Result<()> {
    if abstract_data.is_image() {
        process_image_info(abstract_data)
    } else {
        process_video_info(abstract_data)
    }
}

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
    // from the image either way. Plus the read-only further-data bucket for
    // what neither projection consumed — the same two records, so it costs
    // nothing beyond the map it fills.
    let asset_metadata = asset_metadata_for(&abstract_data.source_path(), record.as_ref().ok());
    abstract_data.tag_mut().extend(asset_metadata.native.tags);
    if abstract_data.description().is_none() {
        abstract_data.set_description(asset_metadata.native.description);
    }
    if abstract_data.rating().is_none() {
        abstract_data.set_rating(asset_metadata.native.rating);
    }
    if let Some(further) = abstract_data.further_metadata_mut() {
        *further = asset_metadata.further;
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
    //
    // No further-data bucket here, and deliberately: a video's `exifVec` is
    // ffprobe's, so a video's bucket would have to be derived from ffprobe's
    // output too rather than from the `ExifTool` read above, and which of the
    // two owns it is not decided. `process_image_info` fills the image's.
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
