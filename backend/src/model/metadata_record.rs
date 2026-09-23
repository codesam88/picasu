//! Metadata-only storage payload for `METADATA_TABLE`.
//!
//! `METADATA_TABLE` is keyed by `asset_id` and stores only user/metadata
//! fields. Every identity field (path, timestamps, trash flag, size, ext,
//! album membership, obj type, ids) lives on [`AssetRecord`]
//! (`ASSET_BY_ID`) and is re-projected onto the wire by
//! [`compose_abstract_data`] — the metadata row is never a source of
//! identity.

use std::collections::{BTreeMap, HashMap, HashSet};

use arrayvec::ArrayString;
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::model::abstract_data::AbstractData;
use crate::model::album::{AlbumCombined, AlbumMetadata, Share};
use crate::model::asset::{AssetKind, AssetRecord};
use crate::model::image::{ImageCombined, ImageMetadata};
use crate::model::object::{ObjectSchema, ObjectType};
use crate::model::response::FileEntry;
use crate::model::video::{VideoCombined, VideoMetadata};

/// Metadata-only value stored in `METADATA_TABLE`, keyed by `asset_id`.
///
/// Deliberately excludes every identity or identity-duplicated field: no
/// `id`, no asset `path` (neither `AssetRecord.path` nor the view's
/// `path`/file entry), no `modified`/`scan_time`/`is_trashed`, no
/// `size`/`ext`, no `album` membership, no `dir_path`, no `obj_type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MetadataRecord {
    Image(ImagePayload),
    Video(VideoPayload),
    Album(AlbumPayload),
}

/// Metadata-only payload for image assets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct ImagePayload {
    pub tags: HashSet<String>,
    pub description: Option<String>,
    pub rating: Option<u8>,
    pub is_favorite: bool,
    pub is_archived: bool,
    pub update_at: i64,
    pub pending: bool,
    pub thumbhash: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub phash: Option<Vec<u8>>,
    pub exif_vec: BTreeMap<String, String>,
}

/// Metadata-only payload for video assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct VideoPayload {
    pub tags: HashSet<String>,
    pub description: Option<String>,
    pub rating: Option<u8>,
    pub is_favorite: bool,
    pub is_archived: bool,
    pub update_at: i64,
    pub pending: bool,
    pub thumbhash: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub exif_vec: BTreeMap<String, String>,
}

/// Metadata-only payload for album assets. `id`, `dir_path`, and
/// `is_trashed` are not stored here — composition fills them from the
/// album's `AssetRecord` (`asset_id`, `path`, `is_trashed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct AlbumPayload {
    pub tags: HashSet<String>,
    pub description: Option<String>,
    pub rating: Option<u8>,
    pub is_favorite: bool,
    pub is_archived: bool,
    pub update_at: i64,
    pub pending: bool,
    pub thumbhash: Option<Vec<u8>>,
    pub title: Option<String>,
    pub created_time: i64,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub last_modified_time: i64,
    pub cover: Option<ArrayString<64>>,
    pub item_count: usize,
    pub item_size: u64,
    pub share_list: HashMap<ArrayString<64>, Share>,
    pub custom_title: Option<String>,
}

impl MetadataRecord {
    /// The stored user tags of this payload.
    pub fn tags(&self) -> &HashSet<String> {
        match self {
            MetadataRecord::Image(img) => &img.tags,
            MetadataRecord::Video(vid) => &vid.tags,
            MetadataRecord::Album(alb) => &alb.tags,
        }
    }
}

/// Extract the metadata-only payload from a composed `AbstractData` view.
///
/// Used at every write to `METADATA_TABLE`: identity fields present on the
/// view (path, ids, `size`/`ext`, album, `dir_path`, trash) are dropped —
/// they are owned by [`AssetRecord`].
pub fn to_metadata_record(data: &AbstractData) -> MetadataRecord {
    match data {
        AbstractData::Image(img) => MetadataRecord::Image(ImagePayload {
            tags: img.object.tags.clone(),
            description: img.object.description.clone(),
            rating: img.object.rating,
            is_favorite: img.object.is_favorite,
            is_archived: img.object.is_archived,
            update_at: img.object.update_at,
            pending: img.object.pending,
            thumbhash: img.object.thumbhash.clone(),
            width: img.metadata.width,
            height: img.metadata.height,
            phash: img.metadata.phash.clone(),
            exif_vec: img.metadata.exif_vec.clone(),
        }),
        AbstractData::Video(vid) => MetadataRecord::Video(VideoPayload {
            tags: vid.object.tags.clone(),
            description: vid.object.description.clone(),
            rating: vid.object.rating,
            is_favorite: vid.object.is_favorite,
            is_archived: vid.object.is_archived,
            update_at: vid.object.update_at,
            pending: vid.object.pending,
            thumbhash: vid.object.thumbhash.clone(),
            width: vid.metadata.width,
            height: vid.metadata.height,
            duration: vid.metadata.duration,
            exif_vec: vid.metadata.exif_vec.clone(),
        }),
        AbstractData::Album(alb) => MetadataRecord::Album(AlbumPayload {
            tags: alb.object.tags.clone(),
            description: alb.object.description.clone(),
            rating: alb.object.rating,
            is_favorite: alb.object.is_favorite,
            is_archived: alb.object.is_archived,
            update_at: alb.object.update_at,
            pending: alb.object.pending,
            thumbhash: alb.object.thumbhash.clone(),
            title: alb.metadata.title.clone(),
            created_time: alb.metadata.created_time,
            start_time: alb.metadata.start_time,
            end_time: alb.metadata.end_time,
            last_modified_time: alb.metadata.last_modified_time,
            cover: alb.metadata.cover,
            item_count: alb.metadata.item_count,
            item_size: alb.metadata.item_size,
            share_list: alb.metadata.share_list.clone(),
            custom_title: alb.metadata.custom_title.clone(),
        }),
    }
}

/// Compose the wire `AbstractData` view from an asset's identity
/// [`AssetRecord`] plus its optional stored [`MetadataRecord`] payload.
///
/// Identity is always taken from the record: the media file entry
/// (`FileEntry { file: path, modified, scan_time, is_trashed }`),
/// album `metadata.id`/`dir_path`/`is_trashed`, `object.id` (display rule:
/// `content_hash.unwrap_or(asset_id)` for media, `asset_id` for albums),
/// `obj_type` from `record.kind`, and size/ext/album membership. Metadata
/// fields come from `meta` when present and matching; otherwise they get
/// the same defaults a payload-less record had before (empty tags/EXIF,
/// zero dimensions, album title unset).
///
/// A payload whose variant does not match `record.kind` is treated as
/// missing.
pub fn compose_abstract_data(record: &AssetRecord, meta: Option<&MetadataRecord>) -> AbstractData {
    let matching = meta.filter(|m| {
        matches!(
            (record.kind, m),
            (AssetKind::Image, MetadataRecord::Image(_))
                | (AssetKind::Video, MetadataRecord::Video(_))
                | (AssetKind::Album, MetadataRecord::Album(_))
        )
    });

    // Display rule: media use their content hash as `object.id` (required
    // for compressed thumbnail path resolution); albums use their asset ID
    // (no thumbnails).
    let display_id = match record.kind {
        AssetKind::Album => record.asset_id,
        _ => record.content_hash.unwrap_or(record.asset_id),
    };
    let obj_type = match record.kind {
        AssetKind::Image => ObjectType::Image,
        AssetKind::Video => ObjectType::Video,
        AssetKind::Album => ObjectType::Album,
    };

    // The media file entry is a view assembled from the record; the record
    // is the sole stored owner of the trash flag.
    let file_entry = FileEntry {
        file: record.path.clone(),
        modified: record.modified,
        scan_time: record.scan_time,
        is_trashed: record.is_trashed,
    };

    match record.kind {
        AssetKind::Image => {
            let mut object = ObjectSchema::new(display_id, obj_type);
            let mut metadata = ImageMetadata::new(record.file_size, 0, 0, record.ext.clone());
            if let Some(MetadataRecord::Image(payload)) = matching {
                object.tags.clone_from(&payload.tags);
                object.description.clone_from(&payload.description);
                object.rating = payload.rating;
                object.is_favorite = payload.is_favorite;
                object.is_archived = payload.is_archived;
                object.update_at = payload.update_at;
                object.pending = payload.pending;
                object.thumbhash.clone_from(&payload.thumbhash);
                metadata.width = payload.width;
                metadata.height = payload.height;
                metadata.phash.clone_from(&payload.phash);
                metadata.exif_vec.clone_from(&payload.exif_vec);
            }
            metadata.album = record.album_id;
            metadata.path = Some(file_entry);
            AbstractData::Image(ImageCombined { object, metadata })
        }
        AssetKind::Video => {
            let mut object = ObjectSchema::new(display_id, obj_type);
            let mut metadata = VideoMetadata::new(record.file_size, 0, 0, record.ext.clone());
            if let Some(MetadataRecord::Video(payload)) = matching {
                object.tags.clone_from(&payload.tags);
                object.description.clone_from(&payload.description);
                object.rating = payload.rating;
                object.is_favorite = payload.is_favorite;
                object.is_archived = payload.is_archived;
                object.update_at = payload.update_at;
                object.pending = payload.pending;
                object.thumbhash.clone_from(&payload.thumbhash);
                metadata.width = payload.width;
                metadata.height = payload.height;
                metadata.duration = payload.duration;
                metadata.exif_vec.clone_from(&payload.exif_vec);
            }
            metadata.album = record.album_id;
            metadata.path = Some(file_entry);
            AbstractData::Video(VideoCombined { object, metadata })
        }
        AssetKind::Album => {
            let mut object = ObjectSchema::new(display_id, obj_type);
            let mut metadata = AlbumMetadata {
                id: record.asset_id,
                dir_path: record.path.clone(),
                is_trashed: record.is_trashed,
                ..Default::default()
            };
            if let Some(MetadataRecord::Album(payload)) = matching {
                object.tags.clone_from(&payload.tags);
                object.description.clone_from(&payload.description);
                object.rating = payload.rating;
                object.is_favorite = payload.is_favorite;
                object.is_archived = payload.is_archived;
                object.update_at = payload.update_at;
                object.pending = payload.pending;
                object.thumbhash.clone_from(&payload.thumbhash);
                metadata.title.clone_from(&payload.title);
                metadata.created_time = payload.created_time;
                metadata.start_time = payload.start_time;
                metadata.end_time = payload.end_time;
                metadata.last_modified_time = payload.last_modified_time;
                metadata.cover = payload.cover;
                metadata.item_count = payload.item_count;
                metadata.item_size = payload.item_size;
                metadata.share_list.clone_from(&payload.share_list);
                metadata.custom_title.clone_from(&payload.custom_title);
            }
            AbstractData::Album(AlbumCombined { object, metadata })
        }
    }
}
