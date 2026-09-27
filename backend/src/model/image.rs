use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::model::object::ObjectSchema;

/// Combined Image data with Object and Metadata
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct ImageCombined {
    #[serde(flatten)]
    pub object: ObjectSchema,
    #[serde(flatten)]
    pub metadata: ImageMetadata,
}

use arrayvec::ArrayString;
use std::collections::BTreeMap;

use crate::model::response::FileEntry;

/// Image-specific metadata
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct ImageMetadata {
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub ext: String,
    pub phash: Option<Vec<u8>>,
    pub album: Option<ArrayString<64>>,
    /// The EXIF family, as `ExifTool` names and prints it.
    pub exif_vec: BTreeMap<String, String>,
    /// Metadata `ExifTool` reported that the app does not model, keyed
    /// `Group:Tag` (`IPTC:By-line`, `XMP-xmp:CreatorTool`). Read-only by
    /// contract: the app does not know what these fields mean well enough to
    /// write one back, so there is no edit path for them. See
    /// `process::xmp::map_further_fields` for which keys land here and why.
    ///
    /// Filled on the image path only — a video's `exifVec` comes from
    /// `ffprobe`, and which source would own a video's bucket is undecided.
    pub further_metadata: BTreeMap<String, String>,
    /// The record's single source path and its timestamps. `None` means the
    /// path was pruned (file gone / stale sweep); path-primary records hold
    /// at most one path.
    pub path: Option<FileEntry>,
}

impl ImageMetadata {
    pub fn new(size: u64, width: u32, height: u32, ext: String) -> Self {
        Self {
            size,
            width,
            height,
            ext,
            phash: None,
            album: None,
            exif_vec: BTreeMap::new(),
            further_metadata: BTreeMap::new(),
            path: None,
        }
    }
}
