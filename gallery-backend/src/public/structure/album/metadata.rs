use arrayvec::ArrayString;
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::share::Share;

/// Album-specific metadata
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct AlbumMetadata {
    pub id: ArrayString<64>,
    pub title: Option<String>,
    pub created_time: i64,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub last_modified_time: i64,
    pub cover: Option<ArrayString<64>>,
    pub item_count: usize,
    pub item_size: u64,
    pub share_list: HashMap<ArrayString<64>, Share>,
    /// Set for filesystem-hierarchy albums; `None` for all user-created albums.
    /// When present, album membership is derived from source file paths rather than
    /// the `albums` set on each media item — the two album types are fully independent.
    pub dir_path: Option<String>,
}
