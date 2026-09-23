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

use crate::model::response::FileModify;

/// Image-specific metadata
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct ImageMetadata {
    pub id: ArrayString<64>,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub ext: String,
    pub phash: Option<Vec<u8>>,
    pub album: Option<ArrayString<64>>,
    pub exif_vec: BTreeMap<String, String>,
    /// The record's single source path and its timestamps. `None` means the
    /// alias was pruned (file gone / stale sweep) — path-primary records
    /// never carry more than one alias.
    pub alias: Option<FileModify>,
}

impl ImageMetadata {
    pub fn new(id: ArrayString<64>, size: u64, width: u32, height: u32, ext: String) -> Self {
        Self {
            id,
            size,
            width,
            height,
            ext,
            phash: None,
            album: None,
            exif_vec: BTreeMap::new(),
            alias: None,
        }
    }
}
