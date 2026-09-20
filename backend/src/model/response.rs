use serde::{Deserialize, Serialize};

use crate::{model::abstract_data::AbstractData, router::auth::ClaimsHash};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseTimestamp {
    pub abstract_data: AbstractData,
    pub timestamp: i64,
    /// The path-primary asset ID for this record. Used to populate
    /// `ReducedData.asset_id` so each physical file is independently addressable.
    #[serde(default)]
    pub asset_id: Option<arrayvec::ArrayString<64>>,
}

impl DatabaseTimestamp {
    pub fn new(abstract_data: AbstractData, priority_list: &[&str]) -> Self {
        let timestamp = abstract_data.compute_timestamp(priority_list);
        Self {
            abstract_data,
            timestamp,
            asset_id: None,
        }
    }

    pub fn with_asset_id(
        abstract_data: AbstractData,
        priority_list: &[&str],
        asset_id: arrayvec::ArrayString<64>,
    ) -> Self {
        let timestamp = abstract_data.compute_timestamp(priority_list);
        Self {
            abstract_data,
            timestamp,
            asset_id: Some(asset_id),
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
    /// Path-primary asset ID. Present when the record was resolved via
    /// the asset tables. Allows the frontend to address specific assets
    /// instead of content hashes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
}

impl DataBaseTimestampReturn {
    #[allow(dead_code)]
    pub fn new(
        abstract_data: AbstractData,
        priority_list: &[&str],
        token_timestamp: i64,
        allow_original: bool,
    ) -> Self {
        let timestamp = abstract_data.compute_timestamp(priority_list);
        let token = match &abstract_data {
            AbstractData::Image(img) => {
                ClaimsHash::new(img.object.id, token_timestamp, allow_original).encode()
            }
            AbstractData::Video(vid) => {
                ClaimsHash::new(vid.object.id, token_timestamp, allow_original).encode()
            }
            AbstractData::Album(alb) => {
                if let Some(cover_hash) = alb.metadata.cover {
                    ClaimsHash::new(cover_hash, token_timestamp, allow_original).encode()
                } else {
                    String::new()
                }
            }
        };
        Self {
            abstract_data,
            timestamp,
            token,
            asset_id: None,
        }
    }

    /// Create with `asset_id` included in the token for path-primary identity.
    pub fn with_asset_id(
        abstract_data: AbstractData,
        priority_list: &[&str],
        token_timestamp: i64,
        allow_original: bool,
        asset_id: arrayvec::ArrayString<64>,
    ) -> Self {
        let timestamp = abstract_data.compute_timestamp(priority_list);
        let token = match &abstract_data {
            AbstractData::Image(img) => {
                ClaimsHash::new(img.object.id, token_timestamp, allow_original)
                    .with_asset_id(asset_id)
                    .encode()
            }
            AbstractData::Video(vid) => {
                ClaimsHash::new(vid.object.id, token_timestamp, allow_original)
                    .with_asset_id(asset_id)
                    .encode()
            }
            AbstractData::Album(_) => {
                ClaimsHash::new(asset_id, token_timestamp, allow_original).encode()
            }
        };
        Self {
            abstract_data,
            timestamp,
            token,
            asset_id: Some(asset_id.to_string()),
        }
    }
}

use arrayvec::ArrayString;
use bitcode::{Decode, Encode};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Decode, Encode)]
pub struct ReducedData {
    pub asset_id: ArrayString<64>,
    pub hash: ArrayString<64>,
    pub width: u32,
    pub height: u32,
    pub date: i64,
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
    /// Per-alias trash flag. A record is visible as long as it has at least one
    /// non-trashed alias, and appears in the trash view while it has at least
    /// one trashed alias. Newly discovered aliases are always live.
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
