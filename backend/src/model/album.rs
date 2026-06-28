use arrayvec::ArrayString;
use bitcode::{Decode, Encode};
use chrono::Utc;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::model::abstract_data::AbstractData;
use crate::model::object::ObjectSchema;
use crate::storage::db::TREE;

/// Combined Album data with Object and Metadata
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct AlbumCombined {
    #[serde(flatten)]
    pub object: ObjectSchema,
    #[serde(flatten)]
    pub metadata: AlbumMetadata,
}

/// A helper struct to hold media item info for album calculations
struct MediaItemInfo {
    hash: ArrayString<64>,
    size: u64,
    thumbhash: Option<Vec<u8>>,
    timestamp: i64,
}

impl AlbumCombined {
    pub fn set_cover(&mut self, cover_data: &AbstractData) {
        self.metadata.cover = Some(cover_data.hash());
        self.object.thumbhash = cover_data.thumbhash().cloned();
    }

    fn set_cover_from_info(&mut self, info: &MediaItemInfo) {
        self.metadata.cover = Some(info.hash);
        self.object.thumbhash.clone_from(&info.thumbhash);
    }

    pub fn self_update(&mut self) {
        let ref_data = TREE.in_memory.read().expect("lock poisoned");

        let album_id = self.object.id;
        let dir_path = self.metadata.dir_path.clone();

        // For dir albums, membership is path-based (file's immediate parent == dir_path).
        // For non-dir albums, membership is stored explicitly in item.metadata.album.
        let belongs_to_album = move |alias: &[crate::model::response::FileModify],
                                     item_album: Option<ArrayString<64>>|
              -> bool {
            if let Some(ref dir) = dir_path {
                let dir_path = Path::new(dir.as_str());
                alias
                    .iter()
                    .any(|a| Path::new(&a.file).parent() == Some(dir_path))
            } else {
                item_album == Some(album_id)
            }
        };

        let mut data_in_album: Vec<MediaItemInfo> = ref_data
            .par_iter()
            .filter_map(
                |database_timestamp| match &database_timestamp.abstract_data {
                    AbstractData::Image(img) => {
                        if !img.object.is_trashed
                            && belongs_to_album(&img.metadata.alias, img.metadata.album)
                        {
                            Some(MediaItemInfo {
                                hash: img.object.id,
                                size: img.metadata.size,
                                thumbhash: img.object.thumbhash.clone(),
                                timestamp: database_timestamp.timestamp,
                            })
                        } else {
                            None
                        }
                    }
                    AbstractData::Video(vid) => {
                        if !vid.object.is_trashed
                            && belongs_to_album(&vid.metadata.alias, vid.metadata.album)
                        {
                            Some(MediaItemInfo {
                                hash: vid.object.id,
                                size: vid.metadata.size,
                                thumbhash: vid.object.thumbhash.clone(),
                                timestamp: database_timestamp.timestamp,
                            })
                        } else {
                            None
                        }
                    }
                    AbstractData::Album(_) => None,
                },
            )
            .collect();

        if data_in_album.is_empty() {
            self.metadata.start_time = None;
            self.metadata.end_time = None;
            self.metadata.cover = None;
            self.object.thumbhash = None;
            self.metadata.item_count = 0;
            self.metadata.item_size = 0;
            return;
        }

        data_in_album.sort_by_key(|info| std::cmp::Reverse(info.timestamp));

        self.metadata.start_time = data_in_album.last().map(|info| info.timestamp);
        self.metadata.end_time = data_in_album.first().map(|info| info.timestamp);
        self.metadata.item_count = data_in_album.len();
        self.metadata.item_size = data_in_album.iter().map(|info| info.size).sum();
        self.metadata.last_modified_time = Utc::now().timestamp_millis();

        if self.metadata.cover.is_none() {
            if let Some(first_info) = data_in_album.first() {
                self.set_cover_from_info(first_info);
            }
        } else {
            let current_cover = self.metadata.cover.expect("cover not set");
            let cover_still_in_album = data_in_album.iter().any(|info| info.hash == current_cover);
            if !cover_still_in_album && let Some(first_info) = data_in_album.first() {
                self.set_cover_from_info(first_info);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use arrayvec::ArrayString;
    use std::path::Path;

    use crate::model::response::FileModify;

    fn belongs_to_album(
        alias: &[FileModify],
        item_album: Option<ArrayString<64>>,
        dir_path: Option<&str>,
        album_id: ArrayString<64>,
    ) -> bool {
        if let Some(dir) = dir_path {
            let dir_path = Path::new(dir);
            alias
                .iter()
                .any(|a| Path::new(&a.file).parent() == Some(dir_path))
        } else {
            item_album == Some(album_id)
        }
    }

    fn alias(paths: &[&str]) -> Vec<FileModify> {
        paths
            .iter()
            .map(|p| FileModify {
                file: p.to_string(),
                modified: 0,
                scan_time: 0,
            })
            .collect()
    }

    #[test]
    fn dir_album_matches_file_inside_dir() {
        let a = alias(&["/photos/vacation/img.jpg"]);
        assert!(belongs_to_album(
            &a,
            None,
            Some("/photos/vacation"),
            ArrayString::new()
        ));
    }

    #[test]
    fn dir_album_does_not_match_file_in_subdirectory() {
        let a = alias(&["/photos/vacation/day1/img.jpg"]);
        assert!(!belongs_to_album(
            &a,
            None,
            Some("/photos/vacation"),
            ArrayString::new()
        ));
    }

    #[test]
    fn child_dir_album_matches_its_own_direct_file() {
        let a = alias(&["/photos/vacation/day1/img.jpg"]);
        assert!(belongs_to_album(
            &a,
            None,
            Some("/photos/vacation/day1"),
            ArrayString::new()
        ));
    }

    #[test]
    fn dir_album_does_not_match_sibling_dir() {
        let a = alias(&["/photos/other/img.jpg"]);
        assert!(!belongs_to_album(
            &a,
            None,
            Some("/photos/vacation"),
            ArrayString::new()
        ));
    }

    #[test]
    fn dir_album_does_not_match_partial_name_prefix() {
        let a = alias(&["/photos/vacation2/img.jpg"]);
        assert!(!belongs_to_album(
            &a,
            None,
            Some("/photos/vacation"),
            ArrayString::new()
        ));
    }

    #[test]
    fn manual_album_matches_stored_id() {
        let id = ArrayString::from("abc").expect("failed to create ArrayString");
        assert!(belongs_to_album(&alias(&[]), Some(id), None, id));
    }

    #[test]
    fn manual_album_does_not_match_different_id() {
        let id = ArrayString::from("abc").expect("failed to create ArrayString");
        let other = ArrayString::from("xyz").expect("failed to create ArrayString");
        assert!(!belongs_to_album(&alias(&[]), Some(other), None, id));
    }
}

use std::collections::HashMap;

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

#[derive(
    Debug,
    Clone,
    Deserialize,
    Default,
    Serialize,
    Decode,
    Encode,
    PartialEq,
    Eq,
    Hash,
    utoipa::ToSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    #[schema(value_type = String)]
    pub url: ArrayString<64>,
    pub description: String,
    pub password: Option<String>,
    pub show_metadata: bool,
    pub show_download: bool,
    pub show_upload: bool,
    pub exp: i64,
}

#[derive(Debug, Clone, Deserialize, Default, Serialize, Decode, Encode, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct ResolvedShare {
    pub share: Share,
    #[schema(value_type = String)]
    pub album_id: ArrayString<64>,
    pub album_title: Option<String>,
}

impl ResolvedShare {
    pub fn new(album_id: ArrayString<64>, album_title: Option<String>, share: Share) -> Self {
        Self {
            share,
            album_id,
            album_title,
        }
    }
}
