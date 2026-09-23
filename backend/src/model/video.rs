use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::model::object::ObjectSchema;

/// Combined Video data with Object and Metadata
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct VideoCombined {
    #[serde(flatten)]
    pub object: ObjectSchema,
    #[serde(flatten)]
    pub metadata: VideoMetadata,
}

use arrayvec::ArrayString;
use std::collections::BTreeMap;

use crate::model::response::FileModify;

/// Video-specific metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub id: ArrayString<64>,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub ext: String,
    pub duration: f64,
    pub album: Option<ArrayString<64>>,
    pub exif_vec: BTreeMap<String, String>,
    /// The record's single source path and its timestamps. `None` means the
    /// path was pruned (file gone / stale sweep); path-primary records hold
    /// at most one path.
    pub path: Option<FileModify>,
}

impl VideoMetadata {
    pub fn new(id: ArrayString<64>, size: u64, width: u32, height: u32, ext: String) -> Self {
        Self {
            id,
            size,
            width,
            height,
            ext,
            duration: 0.0,
            album: None,
            exif_vec: BTreeMap::new(),
            path: None,
        }
    }
}
