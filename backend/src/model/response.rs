use serde::{Deserialize, Serialize};

use crate::{model::abstract_data::AbstractData, router::auth::ClaimsHash};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseTimestamp {
    pub abstract_data: AbstractData,
    pub timestamp: i64,
}

impl DatabaseTimestamp {
    pub fn new(abstract_data: AbstractData, priority_list: &[&str]) -> Self {
        let timestamp = abstract_data.compute_timestamp(priority_list);
        Self {
            abstract_data,
            timestamp,
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
}

impl DataBaseTimestampReturn {
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
        }
    }
}

use arrayvec::ArrayString;
use bitcode::{Decode, Encode};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Decode, Encode)]
pub struct ReducedData {
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

#[derive(Debug, Default, Clone, Deserialize, Serialize, Decode, Encode)]
#[serde(rename_all = "camelCase")]
pub struct FileModify {
    pub file: String,
    pub modified: i64,
    pub scan_time: i64,
}

impl FileModify {
    pub fn new(file: &Path, modified: i64) -> Self {
        Self {
            file: file.to_string_lossy().into_owned(),
            modified,
            scan_time: Utc::now().timestamp_millis(),
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
