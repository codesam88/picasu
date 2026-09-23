use serde::{Deserialize, Serialize};

use crate::{model::abstract_data::AbstractData, router::auth::ClaimsHash};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseTimestamp {
    pub abstract_data: AbstractData,
    pub timestamp: i64,
    /// The path-primary asset ID for this record.
    pub asset_id: arrayvec::ArrayString<64>,
}

impl DatabaseTimestamp {
    pub fn with_asset_id(
        abstract_data: AbstractData,
        priority_list: &[&str],
        asset_id: arrayvec::ArrayString<64>,
    ) -> Self {
        let timestamp = abstract_data.compute_timestamp(priority_list);
        Self {
            abstract_data,
            timestamp,
            asset_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DataBaseTimestampReturn {
    #[schema(value_type = Object)]
    pub abstract_data: AbstractData,
    pub timestamp: i64,
    pub token: String,
    /// Path-primary asset ID.
    pub asset_id: String,
    /// For albums: the cover image's content hash (used for compressed
    /// thumbnail URL construction and token validation). `None` for media
    /// items or albums without a cover.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_hash: Option<String>,
}

impl DataBaseTimestampReturn {
    /// Create with `asset_id` included in the token for path-primary identity.
    /// For albums, `cover_content_hash` is the cover image's `object.id`
    /// (content hash), looked up from `METADATA_TABLE` by the cover's
    /// `asset_id`.  It is used for the token's `hash` claim (`GuardHash`
    /// validation) and exposed to the frontend for compressed thumbnail URL
    /// construction.
    pub fn with_asset_id(
        abstract_data: AbstractData,
        priority_list: &[&str],
        token_timestamp: i64,
        allow_original: bool,
        asset_id: arrayvec::ArrayString<64>,
        cover_content_hash: Option<arrayvec::ArrayString<64>>,
    ) -> Self {
        let timestamp = abstract_data.compute_timestamp(priority_list);
        let token = match &abstract_data {
            AbstractData::Image(img) => {
                ClaimsHash::new(img.object.id, asset_id, token_timestamp, allow_original).encode()
            }
            AbstractData::Video(vid) => {
                ClaimsHash::new(vid.object.id, asset_id, token_timestamp, allow_original).encode()
            }
            AbstractData::Album(album) => {
                // Album cover compressed path uses the cover's content hash.
                // The token must carry that hash so GuardHash can validate it.
                // Set allow_original = false: album covers have no independent
                // original — the asset_id in the token is the album's, not a
                // media asset, so original access would resolve to nothing.
                let token_hash =
                    cover_content_hash.unwrap_or(album.metadata.cover.unwrap_or(asset_id));
                ClaimsHash::new(token_hash, asset_id, token_timestamp, false).encode()
            }
        };
        let cover_hash = cover_content_hash.map(|h| h.to_string());
        Self {
            abstract_data,
            timestamp,
            token,
            asset_id: asset_id.to_string(),
            cover_hash,
        }
    }
}

use arrayvec::ArrayString;
use bitcode::{Decode, Encode};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Decode, Encode)]
pub struct ReducedData {
    /// Path-primary asset ID.
    pub asset_id: ArrayString<64>,
    pub hash: ArrayString<64>,
    pub width: u32,
    pub height: u32,
    pub date: i64,
    /// Stored `object.update_at` — the `updated_at` cache-bust key for image
    /// URLs. Carried in the snapshot so lean list rows preserve it without a
    /// `METADATA_TABLE` read (rotate / regenerate-thumbnail rely on it).
    pub update_at: i64,
    /// Stored `object.pending` — true while a thumbnail/video frame is still
    /// being generated (rendered as the tile "processing" chip).
    pub pending: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Decode, Encode)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct Prefetch {
    pub timestamp: i64,
    pub locate_to: Option<usize>,
    pub data_length: usize,
}

impl Prefetch {
    pub fn new(timestamp: i64, locate_to: Option<usize>, data_length: usize) -> Self {
        Self {
            timestamp,
            locate_to,
            data_length,
        }
    }
}

#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Encode, Decode)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DisplayElement {
    pub display_width: u32,
    pub display_height: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Encode, Decode)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
#[allow(clippy::struct_field_names)]
pub struct Row {
    pub start: usize,
    pub end: usize,
    pub display_elements: Vec<DisplayElement>,
    pub row_index: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct ScrollBarData {
    pub year: usize,
    pub month: usize,
    pub index: usize,
}

use chrono::Utc;

use std::{cmp::Ordering, path::Path};

#[derive(Debug, Default, Clone, Deserialize, Serialize, Decode, Encode, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileModify {
    pub file: String,
    pub modified: i64,
    pub scan_time: i64,
    /// Trash flag for the record's single alias. The record is visible in the
    /// gallery while the alias is live and in the trash view while it is
    /// trashed; a pruned alias (`alias: None`) matches neither view. Newly
    /// discovered aliases are always live.
    pub is_trashed: bool,
}

impl FileModify {
    pub fn new(file: &Path, modified: i64) -> Self {
        Self {
            file: file.to_string_lossy().into_owned(),
            modified,
            scan_time: Utc::now().timestamp_millis(),
            is_trashed: false,
        }
    }
}

impl PartialEq for FileModify {
    fn eq(&self, other: &Self) -> bool {
        self.scan_time == other.scan_time
    }
}
impl Eq for FileModify {}

impl PartialOrd for FileModify {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FileModify {
    fn cmp(&self, other: &Self) -> Ordering {
        self.scan_time.cmp(&other.scan_time)
    }
}

impl std::hash::Hash for FileModify {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.scan_time.hash(state);
    }
}
