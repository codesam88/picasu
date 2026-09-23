use arrayvec::ArrayString;
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// The kind of asset: image, video, or album.
///
/// Albums are typed assets in the same namespace as media files. They do not
/// participate in content-hash duplicate groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub enum AssetKind {
    Image,
    Video,
    Album,
}

impl fmt::Display for AssetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetKind::Image => write!(f, "image"),
            AssetKind::Video => write!(f, "video"),
            AssetKind::Album => write!(f, "album"),
        }
    }
}

impl FromStr for AssetKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "image" => Ok(AssetKind::Image),
            "video" => Ok(AssetKind::Video),
            "album" => Ok(AssetKind::Album),
            _ => Err(format!("Invalid AssetKind: {s}")),
        }
    }
}

impl AssetKind {
    /// Returns `true` for image and video kinds (media assets that may have
    /// content hashes). Albums never have content hashes.
    pub fn is_media(self) -> bool {
        matches!(self, AssetKind::Image | AssetKind::Video)
    }
}

/// A path-primary asset record. Each physical media file or directory has
/// exactly one `AssetRecord` identified by a unique `asset_id`.
///
/// Two files with identical content have separate `asset_id` values and
/// separate `AssetRecord` entries. They share a hash group via `DUPE_INDEX`
/// but are independently addressable, movable, and deletable.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct AssetRecord {
    /// Unique identifier for this asset. Stable across renames and moves.
    pub asset_id: ArrayString<64>,
    /// Whether this is an image, video, or album.
    pub kind: AssetKind,
    /// Filesystem path. For media files this is the absolute path
    /// to the file; for albums it is the directory path. Unique across all
    /// assets.
    pub path: String,
    /// Optional content hash (blake3). Albums do not have content hashes.
    pub content_hash: Option<ArrayString<64>>,
    /// File size in bytes. Zero for albums.
    pub file_size: u64,
    /// File extension (lowercase, without dot). Empty for albums.
    pub ext: String,
    /// Timestamp (millis since epoch) of the file's last modification.
    pub modified: i64,
    /// Timestamp (millis since epoch) when this asset was first indexed.
    pub scan_time: i64,
    /// Per-asset trash flag.
    pub is_trashed: bool,
    /// Album ID this asset belongs to (for media files). Derived from the
    /// parent directory's album asset.
    pub album_id: Option<ArrayString<64>>,
}

impl AssetRecord {
    /// Create a new media asset record with a generated `asset_id`.
    pub fn new_media(
        kind: AssetKind,
        path: String,
        content_hash: Option<ArrayString<64>>,
        file_size: u64,
        ext: String,
        modified: i64,
    ) -> Self {
        assert!(kind.is_media(), "new_media called with non-media kind");
        let asset_id = crate::process::hash::generate_random_hash();
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            asset_id,
            kind,
            path,
            content_hash,
            file_size,
            ext,
            modified,
            scan_time: now,
            is_trashed: false,
            album_id: None,
        }
    }

    /// Create a new album asset record with a generated `asset_id`.
    pub fn new_album(path: String) -> Self {
        let asset_id = crate::process::hash::generate_random_hash();
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            asset_id,
            kind: AssetKind::Album,
            path,
            content_hash: None,
            file_size: 0,
            ext: String::new(),
            modified: now,
            scan_time: now,
            is_trashed: false,
            album_id: None,
        }
    }

    /// Returns `true` if this is a media asset (image or video).
    #[allow(dead_code)] // exercised by unit tests; no production caller yet
    pub fn is_media(&self) -> bool {
        self.kind.is_media()
    }

    /// Returns `true` if this is an album asset.
    #[allow(dead_code)] // exercised by unit tests; no production caller yet
    pub fn is_album(&self) -> bool {
        self.kind == AssetKind::Album
    }
}

/// Normalize a filesystem path to a canonical form for use as an asset key.
///
/// - Converts to absolute path.
/// - Resolves `.` and `..` components without following symlinks.
/// - Preserves the trailing component exactly.
pub fn canonicalize_path(path: &Path, image_home: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        image_home.join(path)
    };
    abs.clean()
}

/// Extension trait for path cleaning (resolves `.` and `..` without following
/// symlinks).
trait PathClean {
    fn clean(&self) -> PathBuf;
}

impl PathClean for Path {
    fn clean(&self) -> PathBuf {
        let mut components = Vec::new();
        for comp in self.components() {
            match comp {
                std::path::Component::ParentDir => {
                    components.pop();
                }
                std::path::Component::CurDir => {}
                other => components.push(other),
            }
        }
        components.iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_kind_is_media_for_image_and_video() {
        assert!(AssetKind::Image.is_media());
        assert!(AssetKind::Video.is_media());
        assert!(!AssetKind::Album.is_media());
    }

    #[test]
    fn asset_kind_display_roundtrip() {
        for kind in [AssetKind::Image, AssetKind::Video, AssetKind::Album] {
            let s = kind.to_string();
            let parsed: AssetKind = s.parse().unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn asset_kind_invalid_string_returns_error() {
        assert!("invalid".parse::<AssetKind>().is_err());
    }

    #[test]
    fn new_media_generates_unique_ids() {
        let a = AssetRecord::new_media(
            AssetKind::Image,
            "/a.jpg".into(),
            None,
            100,
            "jpg".into(),
            0,
        );
        let b = AssetRecord::new_media(
            AssetKind::Image,
            "/b.jpg".into(),
            None,
            200,
            "jpg".into(),
            0,
        );
        assert_ne!(a.asset_id, b.asset_id, "asset IDs must be unique");
    }

    #[test]
    fn new_media_has_no_album() {
        let r = AssetRecord::new_media(
            AssetKind::Image,
            "/test.jpg".into(),
            None,
            100,
            "jpg".into(),
            0,
        );
        assert_eq!(r.album_id, None);
    }

    #[test]
    fn new_album_has_no_hash() {
        let r = AssetRecord::new_album("/photos/vacation".into());
        assert_eq!(r.content_hash, None);
        assert!(r.is_album());
        assert!(!r.is_media());
    }

    #[test]
    fn new_album_has_zero_size_and_empty_ext() {
        let r = AssetRecord::new_album("/photos".into());
        assert_eq!(r.file_size, 0);
        assert_eq!(r.ext, "");
    }

    #[test]
    fn media_with_hash_stores_hash() {
        let hash: ArrayString<64> = "abc123".parse().unwrap();
        let r = AssetRecord::new_media(
            AssetKind::Image,
            "/img.jpg".into(),
            Some(hash),
            500,
            "jpg".into(),
            0,
        );
        assert_eq!(r.content_hash, Some(hash));
    }

    #[test]
    fn canonicalize_path_resolves_dotdot() {
        let base = Path::new("/photos");
        let path = Path::new("/photos/vacation/../beach/img.jpg");
        let result = canonicalize_path(path, base);
        assert_eq!(result, PathBuf::from("/photos/beach/img.jpg"));
    }

    #[test]
    fn canonicalize_path_resolves_dot() {
        let base = Path::new("/photos");
        let path = Path::new("/photos/./vacation/img.jpg");
        let result = canonicalize_path(path, base);
        assert_eq!(result, PathBuf::from("/photos/vacation/img.jpg"));
    }

    #[test]
    fn canonicalize_path_joins_relative_to_base() {
        let base = Path::new("/photos");
        let path = Path::new("vacation/img.jpg");
        let result = canonicalize_path(path, base);
        assert_eq!(result, PathBuf::from("/photos/vacation/img.jpg"));
    }

    #[test]
    fn different_paths_yield_different_canonical_forms() {
        let base = Path::new("/photos");
        let a = Path::new("/photos/vacation/../beach/img.jpg");
        let b = Path::new("/photos/beach/img.jpg");
        assert_eq!(canonicalize_path(a, base), canonicalize_path(b, base));
    }
}
