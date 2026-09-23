use chrono::Utc;
use std::collections::{BTreeMap, HashSet};
use std::fs::metadata;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use arrayvec::ArrayString;
use bitcode::{Decode, Encode};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use rand::RngExt;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::constant::VALID_IMAGE_EXTENSIONS;

/// Regex for parsing timestamps from filenames (e.g., `20231225_143052`)
static FILE_NAME_TIME_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(\d{4})[^a-zA-Z0-9]?(\d{2})[^a-zA-Z0-9]?(\d{2})[^a-zA-Z0-9]?(\d{2})[^a-zA-Z0-9]?(\d{2})[^a-zA-Z0-9]?(\d{2})\b").expect("failed to compile FILE_NAME_TIME_REGEX")
});

use super::{
    album::AlbumCombined,
    image::{ImageCombined, ImageMetadata},
    object::{ObjectSchema, ObjectType},
    response::FileModify,
    video::{VideoCombined, VideoMetadata},
};

/// `AbstractData` enum with Image, Video, and Album variants
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AbstractData {
    Image(ImageCombined),
    Video(VideoCombined),
    Album(AlbumCombined),
}

impl AbstractData {
    /// Get `object.id`: the content hash for media records, the album's asset
    /// ID for album records.
    pub fn hash(&self) -> ArrayString<64> {
        match self {
            AbstractData::Image(img) => img.object.id,
            AbstractData::Video(vid) => vid.object.id,
            AbstractData::Album(alb) => alb.object.id,
        }
    }

    /// Get the width
    pub fn width(&self) -> u32 {
        match self {
            AbstractData::Image(img) => img.metadata.width,
            AbstractData::Video(vid) => vid.metadata.width,
            AbstractData::Album(_) => 300,
        }
    }

    /// Get the height
    pub fn height(&self) -> u32 {
        match self {
            AbstractData::Image(img) => img.metadata.height,
            AbstractData::Video(vid) => vid.metadata.height,
            AbstractData::Album(_) => 300,
        }
    }

    /// Get description
    pub fn description(&self) -> Option<&str> {
        match self {
            AbstractData::Image(img) => img.object.description.as_deref(),
            AbstractData::Video(vid) => vid.object.description.as_deref(),
            AbstractData::Album(alb) => alb.object.description.as_deref(),
        }
    }

    /// Set description
    pub fn set_description(&mut self, description: Option<String>) {
        match self {
            AbstractData::Image(img) => img.object.description = description,
            AbstractData::Video(vid) => vid.object.description = description,
            AbstractData::Album(alb) => alb.object.description = description,
        }
    }

    /// Get rating
    pub fn rating(&self) -> Option<u8> {
        match self {
            AbstractData::Image(img) => img.object.rating,
            AbstractData::Video(vid) => vid.object.rating,
            AbstractData::Album(alb) => alb.object.rating,
        }
    }

    /// Get tags (reference)
    pub fn tag(&self) -> &HashSet<String> {
        match self {
            AbstractData::Image(img) => &img.object.tags,
            AbstractData::Video(vid) => &vid.object.tags,
            AbstractData::Album(alb) => &alb.object.tags,
        }
    }

    /// Get tags (mutable reference)
    pub fn tag_mut(&mut self) -> &mut HashSet<String> {
        match self {
            AbstractData::Image(img) => &mut img.object.tags,
            AbstractData::Video(vid) => &mut vid.object.tags,
            AbstractData::Album(alb) => &mut alb.object.tags,
        }
    }

    /// Update the `update_at` timestamp
    pub fn update_update_at(&mut self) {
        let timestamp = Utc::now().timestamp_millis();
        match self {
            AbstractData::Image(img) => img.object.update_at = timestamp,
            AbstractData::Video(vid) => vid.object.update_at = timestamp,
            AbstractData::Album(alb) => alb.object.update_at = timestamp,
        }
    }

    /// Stored `update_at` value (image-URL cache-bust key).
    pub fn update_at(&self) -> i64 {
        match self {
            AbstractData::Image(img) => img.object.update_at,
            AbstractData::Video(vid) => vid.object.update_at,
            AbstractData::Album(alb) => alb.object.update_at,
        }
    }

    /// Processing flag — true while a thumbnail/video frame is being generated.
    pub fn pending(&self) -> bool {
        match self {
            AbstractData::Image(img) => img.object.pending,
            AbstractData::Video(vid) => vid.object.pending,
            AbstractData::Album(alb) => alb.object.pending,
        }
    }

    /// Compute timestamp for sorting based on priority list
    /// Checks fields in order: `DateTimeOriginal`, filename, `scan_time`, modified, random
    pub fn compute_timestamp(&self, priority_list: &[&str]) -> i64 {
        if let AbstractData::Album(alb) = self {
            return alb.metadata.created_time;
        }

        let now_time = chrono::Local::now().naive_local();
        let exif_vec = self.exif_vec();
        let file_entry = self.path();

        for &field in priority_list {
            match field {
                "DateTimeOriginal" => {
                    if let Some(exif) = exif_vec
                        && let Some(value) = exif.get("DateTimeOriginal")
                        && let Ok(naive_dt) =
                            NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
                        && let Some(local_dt) =
                            chrono::Local.from_local_datetime(&naive_dt).single()
                        && local_dt.naive_local() <= now_time
                    {
                        return local_dt.timestamp_millis();
                    }
                }
                "filename" => {
                    let mut max_time: Option<NaiveDateTime> = None;

                    if let Some(entry) = file_entry {
                        let path = PathBuf::from(&entry.file);

                        if let Some(file_name) = path.file_name()
                            && let Some(file_name_str) = file_name.to_str()
                            && let Some(caps) = FILE_NAME_TIME_REGEX.captures(file_name_str)
                            && let (Ok(year), Ok(month), Ok(day), Ok(hour), Ok(minute), Ok(second)) = (
                                caps[1].parse::<i32>(),
                                caps[2].parse::<u32>(),
                                caps[3].parse::<u32>(),
                                caps[4].parse::<u32>(),
                                caps[5].parse::<u32>(),
                                caps[6].parse::<u32>(),
                            )
                            && let Some(date) = NaiveDate::from_ymd_opt(year, month, day)
                            && let Some(time) = NaiveTime::from_hms_opt(hour, minute, second)
                        {
                            let datetime = NaiveDateTime::new(date, time);

                            if datetime <= now_time {
                                max_time = Some(max_time.map_or(datetime, |t| t.max(datetime)));
                            }
                        }
                    }

                    if let Some(datetime) = max_time {
                        return chrono::Local
                            .from_local_datetime(&datetime)
                            .single()
                            .expect("failed to convert datetime to local timezone")
                            .timestamp_millis();
                    }
                }
                "scan_time" => {
                    let latest_scan_time = file_entry.iter().map(|a| a.scan_time).max();
                    if let Some(latest_time) = latest_scan_time {
                        return latest_time;
                    }
                }
                "modified" => {
                    if let Some(latest_entry) = file_entry.iter().max_by_key(|a| a.scan_time) {
                        return latest_entry.modified;
                    }
                }
                "random" => {
                    let mut rng = rand::rng();
                    let random_number: i64 = rng.random();
                    return random_number;
                }
                _ => panic!("Unknown field type: {field}"),
            }
        }
        0
    }

    /// Get `ext_type` (image/video/album)
    pub fn ext_type(&self) -> &str {
        match self {
            AbstractData::Image(_) => "image",
            AbstractData::Video(_) => "video",
            AbstractData::Album(_) => "album",
        }
    }

    /// Get `exif_vec`
    pub fn exif_vec(&self) -> Option<&BTreeMap<String, String>> {
        match self {
            AbstractData::Image(img) => Some(&img.metadata.exif_vec),
            AbstractData::Video(vid) => Some(&vid.metadata.exif_vec),
            AbstractData::Album(_) => None,
        }
    }

    /// Get `exif_vec` mutable
    pub fn exif_vec_mut(&mut self) -> Option<&mut BTreeMap<String, String>> {
        match self {
            AbstractData::Image(img) => Some(&mut img.metadata.exif_vec),
            AbstractData::Video(vid) => Some(&mut vid.metadata.exif_vec),
            AbstractData::Album(_) => None,
        }
    }

    /// Get the asset's file entry. `None` for albums and for media
    /// records whose path has been pruned (file gone).
    pub fn path(&self) -> Option<&FileModify> {
        match self {
            AbstractData::Image(img) => img.metadata.path.as_ref(),
            AbstractData::Video(vid) => vid.metadata.path.as_ref(),
            AbstractData::Album(_) => None,
        }
    }

    /// Get the single album this item belongs to (None if unassigned)
    pub fn album(&self) -> Option<ArrayString<64>> {
        match self {
            AbstractData::Image(img) => img.metadata.album,
            AbstractData::Video(vid) => vid.metadata.album,
            AbstractData::Album(_) => None,
        }
    }

    /// Set the single album this item belongs to
    pub fn set_album(&mut self, album: Option<ArrayString<64>>) {
        match self {
            AbstractData::Image(img) => img.metadata.album = album,
            AbstractData::Video(vid) => vid.metadata.album = album,
            AbstractData::Album(_) => {}
        }
    }

    /// Get thumbhash
    pub fn thumbhash(&self) -> Option<&Vec<u8>> {
        match self {
            AbstractData::Image(img) => img.object.thumbhash.as_ref(),
            AbstractData::Video(vid) => vid.object.thumbhash.as_ref(),
            AbstractData::Album(alb) => alb.object.thumbhash.as_ref(),
        }
    }

    /// Check if this is an image
    pub fn is_image(&self) -> bool {
        matches!(self, AbstractData::Image(_))
    }

    /// Check if this is a video
    pub fn is_video(&self) -> bool {
        matches!(self, AbstractData::Video(_))
    }

    /// Create a new `AbstractData` from a file path and hash
    pub fn new(path: &Path, hash: ArrayString<64>) -> Result<Self> {
        let ext = path
            .extension()
            .ok_or_else(|| anyhow::anyhow!("File has no extension: {}", path.display()))?
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Extension is not valid UTF-8: {}", path.display()))?
            .to_ascii_lowercase();

        let md = metadata(path)
            .with_context(|| format!("Failed to read metadata: {}", path.display()))?;
        let size = md.len();

        let modified_millis = md
            .modified()?
            .duration_since(UNIX_EPOCH)
            .with_context(|| format!("Modification time is before UNIX_EPOCH: {}", path.display()))?
            .as_millis();
        let modified_millis = i64::try_from(modified_millis).unwrap_or(0);

        let file_entry = FileModify::new(path, modified_millis);
        let obj_type = Self::determine_type(&ext);

        match obj_type {
            ObjectType::Image => {
                let object = ObjectSchema::new(hash, ObjectType::Image);
                let mut metadata = ImageMetadata::new(hash, size, 0, 0, ext);
                metadata.path = Some(file_entry);
                Ok(AbstractData::Image(ImageCombined { object, metadata }))
            }
            ObjectType::Video => {
                let object = ObjectSchema::new(hash, ObjectType::Video);
                let mut metadata = VideoMetadata::new(hash, size, 0, 0, ext);
                metadata.path = Some(file_entry);
                Ok(AbstractData::Video(VideoCombined { object, metadata }))
            }
            ObjectType::Album => Err(anyhow::anyhow!("Cannot create Album from file path")),
        }
    }

    fn determine_type(ext: &str) -> ObjectType {
        if VALID_IMAGE_EXTENSIONS.contains(&ext) {
            ObjectType::Image
        } else {
            ObjectType::Video
        }
    }

    // Path helper methods

    /// Get the source path string (the record's canonical path; empty for
    /// albums and for records whose path has been pruned).
    pub fn source_path_string(&self) -> &str {
        match self {
            AbstractData::Image(img) => img.metadata.path.as_ref().map_or("", |a| a.file.as_str()),
            AbstractData::Video(vid) => vid.metadata.path.as_ref().map_or("", |a| a.file.as_str()),
            AbstractData::Album(_) => "",
        }
    }

    /// Get the source path
    pub fn source_path(&self) -> PathBuf {
        PathBuf::from(self.source_path_string())
    }

    /// Get the compressed path string
    pub fn compressed_path_string(&self) -> String {
        let hash = self.hash();
        let relative_path = match self {
            AbstractData::Image(_) => {
                format!("object/compressed/{}/{}.jpg", &hash.as_str()[0..2], hash)
            }
            AbstractData::Video(_) => {
                format!("object/compressed/{}/{}.mp4", &hash.as_str()[0..2], hash)
            }
            AbstractData::Album(_) => String::new(),
        };

        if relative_path.is_empty() {
            return String::new();
        }

        crate::storage::files::get_data_path()
            .join(relative_path)
            .to_string_lossy()
            .into_owned()
    }

    /// Get the compressed path
    pub fn compressed_path(&self) -> PathBuf {
        PathBuf::from(self.compressed_path_string())
    }

    /// Get the thumbnail path
    pub fn thumbnail_path(&self) -> String {
        let hash = self.hash();
        crate::storage::files::get_data_path()
            .join(format!(
                "object/compressed/{}/{}.jpg",
                &hash.as_str()[0..2],
                hash
            ))
            .to_string_lossy()
            .into_owned()
    }

    /// Get the parent directory of the compressed path
    pub fn compressed_path_parent(&self) -> PathBuf {
        self.compressed_path()
            .parent()
            .expect("Path::new(&output_file_path_string).parent() fail")
            .to_path_buf()
    }

    /// Get mutable access to the path *slot* itself for media records
    /// (`None` for albums). Returns `Some(&mut Option<FileModify>)` even when
    /// the slot is empty so callers can edit, replace, or clear the path;
    /// use [`AbstractData::path`] for read-only field access.
    pub fn path_mut(&mut self) -> Option<&mut Option<FileModify>> {
        match self {
            AbstractData::Image(img) => Some(&mut img.metadata.path),
            AbstractData::Video(vid) => Some(&mut vid.metadata.path),
            AbstractData::Album(_) => None,
        }
    }

    /// Set pending status
    pub fn set_pending(&mut self, pending: bool) {
        match self {
            AbstractData::Image(img) => img.object.pending = pending,
            AbstractData::Video(vid) => vid.object.pending = pending,
            AbstractData::Album(alb) => alb.object.pending = pending,
        }
    }

    /// Set favorite status
    pub fn set_favorite(&mut self, is_favorite: bool) {
        match self {
            AbstractData::Image(img) => img.object.is_favorite = is_favorite,
            AbstractData::Video(vid) => vid.object.is_favorite = is_favorite,
            AbstractData::Album(alb) => alb.object.is_favorite = is_favorite,
        }
    }

    /// Set archived status
    pub fn set_archived(&mut self, is_archived: bool) {
        match self {
            AbstractData::Image(img) => img.object.is_archived = is_archived,
            AbstractData::Video(vid) => vid.object.is_archived = is_archived,
            AbstractData::Album(alb) => alb.object.is_archived = is_archived,
        }
    }

    /// Set trashed status.
    ///
    /// For images/videos the flag lives on the asset's file entry, so this
    /// updates it in place (a missing path carries no trash state). Albums
    /// carry a single record-level flag on `AlbumMetadata`.
    pub fn set_trashed(&mut self, is_trashed: bool) {
        match self {
            AbstractData::Image(img) => {
                if let Some(entry) = img.metadata.path.as_mut() {
                    entry.is_trashed = is_trashed;
                }
            }
            AbstractData::Video(vid) => {
                if let Some(entry) = vid.metadata.path.as_mut() {
                    entry.is_trashed = is_trashed;
                }
            }
            AbstractData::Album(alb) => alb.metadata.is_trashed = is_trashed,
        }
    }

    /// Set rating (0–5); None clears it
    pub fn set_rating(&mut self, rating: Option<u8>) {
        match self {
            AbstractData::Image(img) => img.object.rating = rating,
            AbstractData::Video(vid) => vid.object.rating = rating,
            AbstractData::Album(alb) => alb.object.rating = rating,
        }
    }

    /// Get mutable reference to width
    pub fn set_width(&mut self, width: u32) {
        match self {
            AbstractData::Image(img) => img.metadata.width = width,
            AbstractData::Video(vid) => vid.metadata.width = width,
            AbstractData::Album(_) => {}
        }
    }

    /// Get mutable reference to height
    pub fn set_height(&mut self, height: u32) {
        match self {
            AbstractData::Image(img) => img.metadata.height = height,
            AbstractData::Video(vid) => vid.metadata.height = height,
            AbstractData::Album(_) => {}
        }
    }

    /// Swap width and height
    pub fn swap_width_height(&mut self) {
        match self {
            AbstractData::Image(img) => {
                std::mem::swap(&mut img.metadata.width, &mut img.metadata.height);
            }
            AbstractData::Video(vid) => {
                std::mem::swap(&mut vid.metadata.width, &mut vid.metadata.height);
            }
            AbstractData::Album(_) => {}
        }
    }

    /// Set thumbhash
    pub fn set_thumbhash(&mut self, thumbhash: Vec<u8>) {
        match self {
            AbstractData::Image(img) => img.object.thumbhash = Some(thumbhash),
            AbstractData::Video(vid) => vid.object.thumbhash = Some(thumbhash),
            AbstractData::Album(alb) => alb.object.thumbhash = Some(thumbhash),
        }
    }

    /// Set phash (only for images)
    pub fn set_phash(&mut self, phash: Vec<u8>) {
        if let AbstractData::Image(img) = self {
            img.metadata.phash = Some(phash);
        }
    }
}

impl From<ImageCombined> for AbstractData {
    fn from(image: ImageCombined) -> Self {
        AbstractData::Image(image)
    }
}

impl From<VideoCombined> for AbstractData {
    fn from(video: VideoCombined) -> Self {
        AbstractData::Video(video)
    }
}

impl From<AlbumCombined> for AbstractData {
    fn from(album: AlbumCombined) -> Self {
        AbstractData::Album(album)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::object::ObjectType;

    /// Build an image record with its single path-primary file entry.
    fn img_with_path(file: &str, modified: i64, scan_time: i64) -> AbstractData {
        let id = ArrayString::from("test").expect("failed to create ArrayString");
        let mut metadata = ImageMetadata::new(id, 0, 0, 0, "jpg".to_string());
        metadata.path = Some(FileModify {
            file: file.to_string(),
            modified,
            scan_time,
            is_trashed: false,
        });
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata,
        })
    }

    /// Build an image record whose path has been pruned (`None`).
    fn img_without_path() -> AbstractData {
        let id = ArrayString::from("test").expect("failed to create ArrayString");
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata: ImageMetadata::new(id, 0, 0, 0, "jpg".to_string()),
        })
    }

    fn img_with_exif(key: &str, value: &str) -> AbstractData {
        let id = ArrayString::from("test").expect("failed to create ArrayString");
        let mut metadata = ImageMetadata::new(id, 0, 0, 0, "jpg".to_string());
        metadata.exif_vec.insert(key.to_string(), value.to_string());
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata,
        })
    }

    #[test]
    fn scan_time_returns_path_scan_time() {
        let data = img_with_path("/b.jpg", 200, 2000);
        assert_eq!(data.compute_timestamp(&["scan_time"]), 2000);
    }

    #[test]
    fn modified_returns_path_modified() {
        let data = img_with_path("/b.jpg", 999, 2000);
        assert_eq!(data.compute_timestamp(&["modified"]), 999);
    }

    #[test]
    fn exif_datetime_original_is_parsed_and_returned() {
        let data = img_with_exif("DateTimeOriginal", "2020-06-15 12:30:00");
        let ts = data.compute_timestamp(&["DateTimeOriginal"]);
        assert!(ts > 0, "expected a positive timestamp from EXIF");
    }

    #[test]
    fn invalid_exif_datetime_falls_through_to_next_priority() {
        let data = img_with_path("/img.jpg", 42, 1234);
        // DateTimeOriginal is missing; should fall through to modified
        assert_eq!(
            data.compute_timestamp(&["DateTimeOriginal", "modified"]),
            42
        );
    }

    #[test]
    fn filename_timestamp_is_parsed() {
        // Timestamp must end at a word boundary; the regex uses \b after the last digit group.
        // "20190704_153000.jpg" works: the dot after "00" is a non-word char.
        let data = img_with_path("/Photos/20190704_153000.jpg", 0, 1);
        let ts = data.compute_timestamp(&["filename"]);
        assert!(ts > 0, "expected a positive timestamp parsed from filename");
    }

    #[test]
    fn priority_list_order_is_respected() {
        let mut data = img_with_path("/a.jpg", 55, 999);
        if let AbstractData::Image(ref mut img) = data {
            img.metadata.exif_vec.insert(
                "DateTimeOriginal".to_string(),
                "2021-01-01 00:00:00".to_string(),
            );
        }
        let exif_ts = data.compute_timestamp(&["DateTimeOriginal"]);
        let scan_ts = data.compute_timestamp(&["scan_time"]);
        let combined_ts = data.compute_timestamp(&["DateTimeOriginal", "scan_time"]);
        assert_eq!(combined_ts, exif_ts);
        assert_ne!(combined_ts, scan_ts);
    }

    #[test]
    fn empty_path_scan_time_returns_zero() {
        let data = img_without_path();
        assert_eq!(data.compute_timestamp(&["scan_time"]), 0);
    }

    #[test]
    fn set_trashed_toggles_the_path_trash_flag() {
        let mut data = img_with_path("/a.jpg", 1, 2);
        data.set_trashed(true);
        assert_eq!(data.path().map(|a| a.is_trashed), Some(true));
        data.set_trashed(false);
        assert_eq!(data.path().map(|a| a.is_trashed), Some(false));
    }

    #[test]
    fn set_trashed_on_missing_path_is_a_noop() {
        let mut data = img_without_path();
        data.set_trashed(true);
        assert!(data.path().is_none());
    }
}
