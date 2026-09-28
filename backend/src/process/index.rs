use crate::process::thumbnail::generate_thumbnail_for_image;
use anyhow::{Context, Result};

use crate::model::abstract_data::AbstractData;
use crate::process::exif::{
    exif_map_from_record, generate_exif_for_video, is_toolchain_failure, read_metadata_record,
};
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
/// # Errors
///
/// The pipeline separates two failures that used to be the same stored state. A
/// read that failed **about the file** is absorbed into empty metadata and the
/// asset indexes; a read that failed **about the toolchain** is propagated, so
/// an indexing failure, an upload response or a rebuild's `metadataFailed`
/// counter says the metadata was not read rather than reporting an asset whose
/// emptiness looks like the truth about the file. See
/// [`is_toolchain_failure`] and [`read_image_metadata`].
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

/// The asset's own `ExifTool` record, or the empty answer a read that failed
/// about the file gets.
///
/// The one place the pipeline decides what a read failure means, for both the
/// image and the video path, so the two cannot drift apart:
///
/// * **The toolchain could not be read** — the binary is missing, the session
///   could not start, or the child died and the restart did not help. Nothing
///   was learned about this file and the next file will fail the same way, so
///   the error is returned with the install remedy
///   `process::exif` attached. Every caller of the pipeline already reports a
///   failure per file: the index task logs it and fails, `POST /post/upload`
///   refuses the upload, an album scan counts the file in its `failed` counter
///   and reports `state: failed` when every file failed, and a rebuild records
///   it in `metadata_failed` with this text as the per-file diagnostic.
/// * **This file could not be read** — `ExifTool` ran and rejected the file, or
///   produced output this code could not parse. That is a fact about the file,
///   so the file is the only thing that loses: the record is `None`, every
///   field derived from it is empty, and the asset still indexes. This is what
///   keeps a damaged image an asset rather than a rejected upload, and it is the
///   behaviour `corrupt_exif_in_decodable_image_yields_empty_exif_vec` pins at
///   the API level.
///
/// The video path classifies the same way even though its `exifVec` came from
/// `ffprobe`: the read here is the video's only source of XMP, so absorbing a
/// toolchain failure would store a half-populated record that looks complete.
fn read_image_metadata(abstract_data: &AbstractData) -> Result<Option<serde_json::Value>> {
    match read_metadata_record(&abstract_data.source_path()) {
        Ok(record) => Ok(Some(record)),
        Err(err) if is_toolchain_failure(&err) => Err(err),
        Err(_) => Ok(None),
    }
}

/// Analyse the newly‑imported **image** and populate the `AbstractData` record.
///
/// The `ExifTool` read is the first thing that happens and its failure decides
/// the rest: a toolchain failure returns before a single field is written, so
/// the record cannot be mistaken for a photo with no metadata, and a per-file
/// failure leaves the record empty and lets the pipeline continue.
pub fn process_image_info(abstract_data: &mut AbstractData) -> Result<()> {
    // One `ExifTool` read serves both metadata consumers: the EXIF map and the
    // native fields are two projections of the same record, so a second read
    // would only double the indexing cost.
    let record = read_image_metadata(abstract_data)?;
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
    let asset_metadata = asset_metadata_for(&abstract_data.source_path(), record.as_ref());
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
///
/// The `ffprobe` read is the first thing that happens and a toolchain failure
/// there already fails the pipeline; the `ExifTool` read below is subject to the
/// same split, so a broken metadata toolchain cannot leave a video carrying a
/// populated `exifVec` and no XMP.
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
    // No further-data bucket for a video, by decision (`.plan/
    // test-exif-xmp-handling.md`, "Additional Known Scope Gaps"). `exifVec`
    // above is already ffprobe's whole output — format, streams and tags
    // flattened into one map — so a video's bucket could only be carved back
    // out of it, putting one read-only surface in two sections with nothing
    // reading the difference. That is not the image path's situation, and the
    // image path is the one that has the bucket: its `exifVec` is the EXIF
    // family alone, which is what leaves something over for a bucket.
    // `process_image_info` fills the image's. Revisit if video extraction is
    // ever split the way the image path's is.
    let record = read_image_metadata(abstract_data)?;
    let native = native_metadata_for(&abstract_data.source_path(), record.as_ref());
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

#[cfg(test)]
mod tests {
    use super::{process_image_info, process_media_info, process_video_info};
    use crate::model::abstract_data::AbstractData;
    use crate::process::exif::{ReadSeam, corrupt_exif_byte_order, with_read_seam};
    use crate::tests::bootstrap::*;
    use arrayvec::ArrayString;
    use exiftool::ExifToolError;
    use serde_json::Value;
    use std::path::{Path, PathBuf};

    /// A path no `exiftool` can be spawned from, so the session-start path fails
    /// for the real reason rather than by a test having taken the binary off
    /// `PATH`.
    const MISSING_EXIFTOOL: &str = "/nonexistent/picasu-no-such-exiftool";

    /// The error `ExifTool` reports for a file it will not parse. Measured on
    /// ExifTool 13.59 for a JPEG whose APP1 EXIF segment carries an invalid
    /// byte-order word — a file whose pixels are still perfectly decodable, so
    /// the pipeline's decision to keep it is the whole question.
    fn file_rejection() -> ExifToolError {
        ExifToolError::ExifToolProcess {
            message: "Error: Malformed APP1 EXIF segment".to_string(),
            std_err: "Error: Malformed APP1 EXIF segment".to_string(),
            command_args: "-json -G1 -d %Y-%m-%d %H:%M:%S broken.jpg".to_string(),
        }
    }

    /// A hash `AbstractData::new` accepts, standing in for the one the index
    /// tasks would have computed.
    fn a_hash() -> ArrayString<64> {
        ArrayString::from("b".repeat(64).as_str()).expect("64 characters is an ArrayString<64>")
    }

    /// Every test here takes `TEST_SERIAL_GUARD` because the pipeline writes its
    /// thumbnail under the shared data path, and a scenario running
    /// `reset_backend_state` between the directory creation and the write would
    /// otherwise delete it underneath the test. The source images are in private
    /// temp dirs, so the guard is only ever about the data path.
    fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
        TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Write a generated JPEG into `dir` and return its path.
    ///
    /// A plain temp dir, not `image_home`: the pipeline only ever *reads* the
    /// source and writes its thumbnail to a content-addressed path under the
    /// data path, so a temp dir keeps these tests out of the filesystem the
    /// rebuild tests walk — a test that panics here would otherwise leave files
    /// behind and fail every rebuild test after it with a count mismatch.
    fn a_jpeg(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(path.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            // Above snapfab's `minimal` threshold, so the requested size is the
            // size the file decodes to and the dimension assertions below are
            // about the pipeline's decode rather than about a fixed 2x2.
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            further_iptc: None,
            minimal: false,
        }])
        .expect("generate jpeg");
        path
    }

    /// `path` as a fresh `AbstractData`, which requires the file to exist: the
    /// record is built from a stat of the source.
    fn record_for(path: &Path) -> AbstractData {
        AbstractData::new(path, a_hash()).expect("build AbstractData")
    }

    /// The install remedy an operator has to be given, checked as substrings
    /// because the wording is documentation and the *presence* is the contract.
    fn assert_names_the_remedy(rendered: &str) {
        for expected in [
            "exiftool",
            "just install-exiftool",
            "apt-get install libimage-exiftool-perl",
        ] {
            assert!(
                rendered.contains(expected),
                "a hard metadata failure must be actionable, and has to mention {expected:?}: \
                 {rendered}"
            );
        }
    }

    /// A deployment without a usable `exiftool` must fail the pipeline, not
    /// produce an asset that looks like a photo with no metadata.
    ///
    /// This is the gap: the indexer absorbed every read failure into an empty
    /// map, so a missing binary stored `exifVec: {}` and empty native fields for
    /// every image in the library — the same stored state a photo with no EXIF
    /// produces, and the only signal was one log line per image. The failure has
    /// to leave the pipeline instead, carrying the remedy, so the existing
    /// per-file failure plumbing sees it: the index task errors, an upload is
    /// refused, an album scan counts the file as failed, and a rebuild counts it
    /// in `metadataFailed`.
    ///
    /// The executable is redirected per thread rather than by emptying `PATH`,
    /// which is process-wide and would break every other test in the binary.
    #[test]
    fn a_missing_exiftool_fails_the_image_pipeline() {
        let _guard = serial_guard();
        let _ = &*TEST_ENV;

        let dir = tempfile::tempdir().expect("temp dir");
        let photo = a_jpeg(dir.path(), "photo.jpg");
        let mut data = record_for(&photo);

        let outcome = with_read_seam(
            ReadSeam::Executable(PathBuf::from(MISSING_EXIFTOOL)),
            || process_image_info(&mut data),
        );

        let err = outcome.expect_err(
            "a deployment whose exiftool cannot start must not be indexed as a photo with no \
             metadata",
        );
        let rendered = format!("{err:#}");
        assert_names_the_remedy(&rendered);
        assert!(
            rendered.contains(&photo.to_string_lossy().to_string()),
            "the diagnostic has to name the file it stopped on: {rendered}"
        );
    }

    /// The same failure through the dispatching entry point, because that is
    /// what every orchestration mode actually calls.
    #[test]
    fn a_missing_exiftool_fails_the_shared_media_pipeline() {
        let _guard = serial_guard();
        let _ = &*TEST_ENV;

        let dir = tempfile::tempdir().expect("temp dir");
        let photo = a_jpeg(dir.path(), "photo.jpg");
        let mut data = record_for(&photo);

        let outcome = with_read_seam(
            ReadSeam::Executable(PathBuf::from(MISSING_EXIFTOOL)),
            || process_media_info(&mut data),
        );

        let err = outcome.expect_err("process_media_info is what index and rebuild both call");
        assert_names_the_remedy(&format!("{err:#}"));
    }

    /// A video's `exifVec` comes from `ffprobe`, so the `ExifTool` read is only
    /// its XMP. Absorbing a toolchain failure there would leave a video indexed
    /// with a populated `exifVec` and no XMP at all — the deployment failure
    /// hiding behind a partially-filled record, which is harder to spot than an
    /// empty one.
    #[test]
    fn a_missing_exiftool_fails_the_video_pipeline_too() {
        let _guard = serial_guard();
        let _ = &*TEST_ENV;

        let video = crate::process::exif::pinned_fixture_path("mp4-48x32-ffprobe");
        let mut data = record_for(&video);
        assert!(
            data.is_video(),
            "control: the fixture is indexed as a video, so this exercises the video path"
        );

        let outcome = with_read_seam(
            ReadSeam::Executable(PathBuf::from(MISSING_EXIFTOOL)),
            || process_video_info(&mut data),
        );

        let err = outcome.expect_err("a video must not index either when the toolchain is broken");
        assert_names_the_remedy(&format!("{err:#}"));
    }

    /// A file `ExifTool` rejects is the opposite case and must keep its
    /// behaviour: the file is real, the deployment is fine, and the asset is
    /// worth having with the metadata the file did yield. So the pipeline
    /// reports success with an empty `exifVec` — the contract
    /// `corrupt_exif_in_decodable_image_yields_empty_exif_vec` asserts at the
    /// API level.
    ///
    /// The read error is injected because ExifTool 13.59 does not produce one
    /// here: measured, a JPEG with a malformed EXIF segment is reported *inside*
    /// the JSON record as `ExifTool:Warning` with a parseable record and exit
    /// code 0, so a real damaged file never reaches the classification at all.
    /// The second half of this test therefore covers the shape a damaged file
    /// does produce — a successful read of a record with no EXIF group — and both
    /// halves have to stay empty.
    #[test]
    fn a_file_exiftool_rejects_still_indexes_with_empty_metadata() {
        let _guard = serial_guard();
        let _ = &*TEST_ENV;

        let dir = tempfile::tempdir().expect("temp dir");
        let photo = a_jpeg(dir.path(), "photo.jpg");
        let mut data = record_for(&photo);

        with_read_seam(ReadSeam::Injected(Err(file_rejection())), || {
            process_image_info(&mut data)
                .expect("a file ExifTool rejects is a fact about that file, not a failure");
        });

        assert_eq!(
            data.exif_vec().map(|exif| exif.len()),
            Some(0),
            "the rejected read is an empty exifVec, not a partial one"
        );
        assert_eq!(
            data.tag().len(),
            0,
            "the native fields come from the same failed read and are empty with it"
        );
        assert_eq!(
            (data.width(), data.height()),
            (8, 8),
            "and the rest of the pipeline still ran: a soft failure must not stop the decode"
        );

        // The same asset built from bytes whose EXIF really is unparsable: the
        // read succeeds with a record carrying no EXIF group, which is the shape
        // a damaged file produces in practice.
        let mut damaged_bytes = std::fs::read(&photo).expect("read the generated jpeg");
        corrupt_exif_byte_order(&mut damaged_bytes);
        let damaged = dir.path().join("damaged.jpg");
        std::fs::write(&damaged, &damaged_bytes).expect("write the damaged jpeg");
        let mut damaged_data = record_for(&damaged);

        process_image_info(&mut damaged_data)
            .expect("a decodable image with an unreadable EXIF block must still index");
        assert_eq!(
            damaged_data.exif_vec().map(|exif| exif.len()),
            Some(0),
            "a malformed EXIF block yields no fields"
        );
        assert_eq!(
            (damaged_data.width(), damaged_data.height()),
            (8, 8),
            "the image is still a decodable image"
        );
    }

    /// A record injected as a *success* is the other half of the seam's value:
    /// the pipeline must project it rather than ignore it, or a test could
    /// "prove" the soft path by injecting a failure the pipeline never sees.
    #[test]
    fn an_injected_record_is_the_one_the_pipeline_uses() {
        let _guard = serial_guard();
        let _ = &*TEST_ENV;

        let dir = tempfile::tempdir().expect("temp dir");
        let photo = a_jpeg(dir.path(), "photo.jpg");
        let mut data = record_for(&photo);

        let record: Value = serde_json::json!({
            "SourceFile": photo.to_string_lossy(),
            "IFD0:Make": "InjectedCam",
            "IFD0:Model": "InjectedModel",
            "ExifIFD:DateTimeOriginal": "2024-05-06 07:08:09",
            "XMP-dc:Subject": ["injected-tag"],
            "XMP-dc:Description": "injected description",
        });
        with_read_seam(ReadSeam::Injected(Ok(record)), || {
            process_image_info(&mut data).expect("an injected record is a readable file");
        });

        assert_eq!(
            data.exif_vec().and_then(|exif| exif.get("Model").cloned()),
            Some("InjectedModel".to_string()),
            "the injected record is what reached exifVec: {exif:?}",
            exif = data.exif_vec()
        );
        assert!(
            data.tag().contains("injected-tag"),
            "and what reached the native fields: {tags:?}",
            tags = data.tag()
        );
        assert_eq!(
            data.description(),
            Some("injected description"),
            "which is what makes the assertions in the other direction meaningful"
        );
    }
}
