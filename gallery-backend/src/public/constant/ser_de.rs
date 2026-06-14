use crate::router::get::get_prefetch::Prefetch;

use crate::public::structure::{
    abstract_data::AbstractData,
    album::{Album, combined::AlbumCombined, metadata::AlbumMetadata, share::Share},
    image::ImageCombined,
    object::ObjectSchema,
    response::reduced_data::ReducedData,
    response::row::Row,
    video::VideoCombined,
};
use arrayvec::ArrayString;
use redb::{TypeName, Value};
use std::collections::HashMap;

// ── AbstractData versioned encoding ───────────────────────────────────────────
//
// Every AbstractData record on disk is prefixed with two bytes: [0xFF, version].
//
// 0xFF is safe as a magic marker because AbstractData is a 3-variant enum;
// bitcode encodes its discriminant in the lowest 2 bits of the first byte
// (values 0, 1, 2).  A first byte of 0xFF has bits [1:0] = 11 = discriminant 3,
// which is invalid for this enum — so no legitimately encoded AbstractData record
// can start with 0xFF.
//
// Records written before this versioning system was introduced have no prefix.
// They carry the schema that corresponds to SCHEMA_VERSION 1, so the fallback
// path in from_bytes treats them as version 1.
//
// When the schema changes (new fields, removed fields, reordered variants):
//   1. Increment SCHEMA_VERSION.
//   2. Copy the current structs to AbstractDataVN / AlbumCombinedVN / etc.
//   3. Add a match arm for the old version in from_bytes.

const SCHEMA_VERSION: u8 = 2;

// ── v1 schema types (AlbumMetadata without dir_path) ──────────────────────────

#[derive(bitcode::Decode)]
#[cfg_attr(test, derive(bitcode::Encode))]
struct AlbumMetadataV1 {
    id: ArrayString<64>,
    title: Option<String>,
    created_time: i64,
    start_time: Option<i64>,
    end_time: Option<i64>,
    last_modified_time: i64,
    cover: Option<ArrayString<64>>,
    item_count: usize,
    item_size: u64,
    share_list: HashMap<ArrayString<64>, Share>,
}

#[derive(bitcode::Decode)]
#[cfg_attr(test, derive(bitcode::Encode))]
struct AlbumCombinedV1 {
    object: ObjectSchema,
    metadata: AlbumMetadataV1,
}

#[derive(bitcode::Decode)]
#[cfg_attr(test, derive(bitcode::Encode))]
enum AbstractDataV1 {
    Image(ImageCombined),
    Video(VideoCombined),
    Album(AlbumCombinedV1),
}

impl From<AbstractDataV1> for AbstractData {
    fn from(v1: AbstractDataV1) -> Self {
        match v1 {
            AbstractDataV1::Image(img) => AbstractData::Image(img),
            AbstractDataV1::Video(vid) => AbstractData::Video(vid),
            AbstractDataV1::Album(alb) => AbstractData::Album(AlbumCombined {
                object: alb.object,
                metadata: AlbumMetadata {
                    id: alb.metadata.id,
                    title: alb.metadata.title,
                    created_time: alb.metadata.created_time,
                    start_time: alb.metadata.start_time,
                    end_time: alb.metadata.end_time,
                    last_modified_time: alb.metadata.last_modified_time,
                    cover: alb.metadata.cover,
                    item_count: alb.metadata.item_count,
                    item_size: alb.metadata.item_size,
                    share_list: alb.metadata.share_list,
                    dir_path: None,
                },
            }),
        }
    }
}

impl Value for AbstractData {
    type SelfType<'a>
        = Self
    where
        Self: 'a;
    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        if data.first() == Some(&0xFF) {
            let version = data[1];
            let payload = &data[2..];
            match version {
                1 => AbstractData::from(
                    bitcode::decode::<AbstractDataV1>(payload)
                        .expect("Failed to decode AbstractData v1"),
                ),
                2 => bitcode::decode::<AbstractData>(payload)
                    .expect("Failed to decode AbstractData v2"),
                v => panic!("Unknown AbstractData schema version {v}"),
            }
        } else {
            // Record written before the versioning system was introduced.
            // Its schema is identical to version 1 (no dir_path on AlbumMetadata).
            AbstractData::from(
                bitcode::decode::<AbstractDataV1>(data)
                    .expect("Failed to decode AbstractData (unversioned legacy)"),
            )
        }
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a> {
        let mut out = vec![0xFF, SCHEMA_VERSION];
        out.extend(bitcode::encode(value));
        out
    }

    fn type_name() -> TypeName {
        TypeName::new("AbstractData")
    }
}

impl Value for ReducedData {
    type SelfType<'a>
        = Self
    where
        Self: 'a;
    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }
    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        bitcode::decode::<ReducedData>(data)
            .map_err(|e| {
                error!("Failed to deserialize ReducedData: {:?}", e);
                e
            })
            .unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a> {
        bitcode::encode(value)
    }

    fn type_name() -> TypeName {
        TypeName::new("ReducedData")
    }
}

impl Value for Row {
    type SelfType<'a>
        = Self
    where
        Self: 'a;
    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }
    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        bitcode::decode::<Self>(data).expect("Failed to deserialize Row")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a> {
        bitcode::encode(value)
    }

    fn type_name() -> TypeName {
        TypeName::new("Row")
    }
}

impl Value for Album {
    type SelfType<'a>
        = Self
    where
        Self: 'a;
    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }
    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        bitcode::decode::<Self>(data).expect("Failed to deserialize Album")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a> {
        bitcode::encode(value)
    }

    fn type_name() -> TypeName {
        TypeName::new("Album")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public::structure::{
        image::{combined::ImageCombined, metadata::ImageMetadata},
        object::ObjectType,
    };

    fn make_image_v2() -> AbstractData {
        let id = ArrayString::from("test").unwrap();
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata: ImageMetadata::new(id, 1024, 800, 600, "jpg".to_string()),
        })
    }

    fn make_album_v1() -> AbstractDataV1 {
        let id = ArrayString::from("alb").unwrap();
        AbstractDataV1::Album(AlbumCombinedV1 {
            object: ObjectSchema::new(id, ObjectType::Album),
            metadata: AlbumMetadataV1 {
                id,
                title: Some("Holiday".to_string()),
                created_time: 1000,
                start_time: Some(500),
                end_time: Some(2000),
                last_modified_time: 1500,
                cover: None,
                item_count: 3,
                item_size: 9000,
                share_list: HashMap::new(),
            },
        })
    }

    #[test]
    fn v2_round_trip_image() {
        let original = make_image_v2();
        let bytes = AbstractData::as_bytes(&original);
        let decoded = AbstractData::from_bytes(&bytes);
        match (original, decoded) {
            (AbstractData::Image(orig), AbstractData::Image(dec)) => {
                assert_eq!(orig.object.id, dec.object.id);
                assert_eq!(orig.metadata.ext, dec.metadata.ext);
            }
            _ => panic!("variant mismatch after v2 round-trip"),
        }
    }

    #[test]
    fn v2_bytes_have_correct_prefix() {
        let bytes = AbstractData::as_bytes(&make_image_v2());
        assert_eq!(bytes[0], 0xFF, "magic marker must be 0xFF");
        assert_eq!(bytes[1], 2, "version byte must match SCHEMA_VERSION");
    }

    #[test]
    fn v1_album_migrates_dir_path_to_none() {
        // Encode a v1 album record and verify it is promoted with dir_path = None.
        let mut bytes = vec![0xFF, 1u8];
        bytes.extend(bitcode::encode(&make_album_v1()));

        let decoded = AbstractData::from_bytes(&bytes);
        match decoded {
            AbstractData::Album(alb) => {
                assert_eq!(alb.metadata.title, Some("Holiday".to_string()));
                assert_eq!(alb.metadata.item_count, 3);
                assert_eq!(alb.metadata.dir_path, None);
            }
            _ => panic!("expected Album variant after v1 migration"),
        }
    }

    #[test]
    fn legacy_unversioned_decodes_as_v1_schema() {
        // A record with no 0xFF prefix is treated as the legacy v1 schema.
        let id = ArrayString::from("img").unwrap();
        let v1_img = AbstractDataV1::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata: ImageMetadata::new(id, 0, 0, 0, "png".to_string()),
        });
        let bytes = bitcode::encode(&v1_img); // no versioning prefix
        assert_ne!(bytes[0], 0xFF, "legacy record must not start with 0xFF");

        let decoded = AbstractData::from_bytes(&bytes);
        match decoded {
            AbstractData::Image(img) => assert_eq!(img.metadata.ext, "png"),
            _ => panic!("expected Image variant from legacy bytes"),
        }
    }

    #[test]
    #[should_panic(expected = "Unknown AbstractData schema version")]
    fn unknown_version_panics() {
        AbstractData::from_bytes(&[0xFF, 99, 0, 0, 0]);
    }
}

impl Value for Prefetch {
    type SelfType<'a>
        = Self
    where
        Self: 'a;
    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }
    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        bitcode::decode::<Self>(data).expect("Failed to deserialize Prefetch")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a> {
        bitcode::encode(value)
    }

    fn type_name() -> TypeName {
        TypeName::new("Prefetch")
    }
}
