use crate::model::response::Prefetch;

use crate::model::{
    abstract_data::AbstractData,
    metadata_record::MetadataRecord,
    response::{ReducedData, Row},
};
use redb::{TypeName, Value};

// ── Versioned bitcode encoding for stored enum records ────────────────────────
//
// Every `AbstractData` / `MetadataRecord` record on disk is prefixed with two
// bytes: [0xFF, version].
//
// 0xFF is safe as a magic marker because both types are 3-variant enums;
// bitcode encodes the discriminant in the lowest 2 bits of the first byte
// (values 0, 1, 2).  A first byte of 0xFF has bits [1:0] = 11 = discriminant 3,
// which is invalid for these enums — so no legitimately encoded record can
// start with 0xFF.
//
// The structs in `model/` ARE the schema, and the SCHEMA_VERSION constants
// below are their on-disk version. Migration between schema versions is not
// supported: a record with any other version byte — or with no [0xFF, version]
// prefix at all — cannot be decoded, and `from_bytes` aborts with instructions
// to rebuild (POST /post/rebuild or a fresh DATA_HOME).
//
// When the schema changes (new fields, removed fields, reordered variants):
//   1. Increment the schema version constant.
//   2. Copy the current structs to AbstractDataVN / AlbumCombinedVN / etc.
//   3. Add a match arm for the old version in from_bytes.

const SCHEMA_VERSION: u8 = 2;

/// On-disk schema version for `MetadataRecord` (the `METADATA_TABLE` value).
/// The table's on-disk name changed when `MetadataRecord` replaced
/// `AbstractData` as the stored value, so rows written under the previous
/// name are never decoded by this impl.
const METADATA_SCHEMA_VERSION: u8 = 2;

/// Abort decoding of a record this build cannot interpret, with instructions
/// for the operator.
///
/// redb 4.2's `Value::from_bytes` returns `SelfType<'a>` directly — the trait
/// has no associated `Error` type — so a decode failure can only surface as a
/// panic, the same mechanism every other `Value` impl in this file uses.
fn rebuild_required(type_name: &str, reason: &str) -> ! {
    panic!(
        "Cannot decode {type_name} record: {reason}. \
         Migration between schema versions is not supported; \
         rebuild the database via POST /post/rebuild, \
         or start with a fresh DATA_HOME."
    );
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
        let [0xFF, version, payload @ ..] = data else {
            rebuild_required(
                "AbstractData",
                "missing [0xFF, version] schema prefix \
                 (record predates schema versioning, is truncated, \
                 or was written by an unknown writer)",
            );
        };
        if *version != SCHEMA_VERSION {
            rebuild_required(
                "AbstractData",
                &format!(
                    "unsupported schema version {version} \
                     (this build reads schema version {SCHEMA_VERSION})"
                ),
            );
        }
        bitcode::decode::<AbstractData>(payload).expect("Failed to decode AbstractData")
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

impl Value for MetadataRecord {
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
        let [0xFF, version, payload @ ..] = data else {
            rebuild_required(
                "MetadataRecord",
                "missing [0xFF, version] schema prefix \
                 (record predates schema versioning, is truncated, \
                 or was written by an unknown writer)",
            );
        };
        if *version != METADATA_SCHEMA_VERSION {
            rebuild_required(
                "MetadataRecord",
                &format!(
                    "unsupported schema version {version} \
                     (this build reads schema version {METADATA_SCHEMA_VERSION})"
                ),
            );
        }
        bitcode::decode::<MetadataRecord>(payload).expect("Failed to decode MetadataRecord")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a> {
        let mut out = vec![0xFF, METADATA_SCHEMA_VERSION];
        out.extend(bitcode::encode(value));
        out
    }

    fn type_name() -> TypeName {
        TypeName::new("MetadataRecord")
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
            .expect("failed to deserialize ReducedData")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        image::{ImageCombined, ImageMetadata},
        object::{ObjectSchema, ObjectType},
    };
    use arrayvec::ArrayString;

    fn make_image() -> AbstractData {
        let id = ArrayString::from("test").expect("failed to create test ArrayString");
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata: ImageMetadata::new(1024, 800, 600, "jpg".to_string()),
        })
    }

    #[test]
    fn round_trip_image() {
        let original = make_image();
        let bytes = AbstractData::as_bytes(&original);
        let decoded = AbstractData::from_bytes(&bytes);
        match (original, decoded) {
            (AbstractData::Image(orig), AbstractData::Image(dec)) => {
                assert_eq!(orig.object.id, dec.object.id);
                assert_eq!(orig.metadata.ext, dec.metadata.ext);
            }
            _ => panic!("variant mismatch after round-trip"),
        }
    }

    #[test]
    fn bytes_carry_schema_version_prefix() {
        let bytes = AbstractData::as_bytes(&make_image());
        assert_eq!(bytes[0], 0xFF, "magic marker must be 0xFF");
        assert_eq!(
            bytes[1], SCHEMA_VERSION,
            "version byte must match SCHEMA_VERSION"
        );
        assert_eq!(SCHEMA_VERSION, 2, "current schema is version 2");
    }

    #[test]
    #[should_panic(expected = "POST /post/rebuild")]
    fn unknown_version_requires_rebuild() {
        AbstractData::from_bytes(&[0xFF, 9, 0, 0, 0]);
    }

    /// Rows written before the favorite/archived removal carry the older
    /// schema version on both stored types; they must be refused with
    /// rebuild instructions rather than surfacing as a bare bitcode decode
    /// failure.
    #[test]
    #[should_panic(expected = "POST /post/rebuild")]
    fn pre_removal_object_version_requires_rebuild() {
        AbstractData::from_bytes(&[0xFF, 1, 0, 0, 0]);
    }

    #[test]
    #[should_panic(expected = "POST /post/rebuild")]
    fn pre_removal_metadata_version_requires_rebuild() {
        MetadataRecord::from_bytes(&[0xFF, 1, 0, 0, 0]);
    }

    #[test]
    fn metadata_bytes_carry_schema_version_prefix() {
        let record = crate::model::metadata_record::to_metadata_record(&make_image());
        let bytes = MetadataRecord::as_bytes(&record);
        assert_eq!(bytes[0], 0xFF, "magic marker must be 0xFF");
        assert_eq!(
            bytes[1], METADATA_SCHEMA_VERSION,
            "version byte must match METADATA_SCHEMA_VERSION"
        );
    }

    #[test]
    #[should_panic(expected = "POST /post/rebuild")]
    fn prefixless_record_requires_rebuild() {
        // Current-schema payload with the version prefix stripped. Must NOT
        // fall through to decoding it as one of the versioned schemas: no
        // version byte means the record is unreadable, so silently decoding
        // ancient prefixless bytes would corrupt data.
        let bytes = bitcode::encode(&make_image());
        assert_ne!(
            bytes.first(),
            Some(&0xFF),
            "fixture must not carry the magic prefix"
        );
        AbstractData::from_bytes(&bytes);
    }

    #[test]
    #[should_panic(expected = "POST /post/rebuild")]
    fn magic_without_version_byte_requires_rebuild() {
        // Truncated prefix: magic marker but no version byte.
        AbstractData::from_bytes(&[0xFF]);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::model::{
        image::{ImageCombined, ImageMetadata},
        object::{ObjectSchema, ObjectType},
    };
    use arrayvec::ArrayString;
    use redb::{Database, ReadableDatabase, TableDefinition};

    const TYPED_TABLE: TableDefinition<&str, AbstractData> = TableDefinition::new("data");

    #[test]
    fn image_round_trips_through_redb() {
        let id = ArrayString::from("img").expect("failed to create test ArrayString");
        let original = AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata: ImageMetadata::new(1024, 800, 600, "jpg".to_string()),
        });

        let dir = tempfile::tempdir().expect("failed to create temp directory");
        let db =
            Database::create(dir.path().join("test.redb")).expect("failed to create test database");

        {
            let txn = db.begin_write().expect("failed to begin write transaction");
            let mut table = txn
                .open_table(TYPED_TABLE)
                .expect("failed to open typed table");
            table
                .insert("img", original)
                .expect("failed to insert test record");
            drop(table);
            txn.commit().expect("failed to commit transaction");
        }

        let txn = db.begin_read().expect("failed to begin read transaction");
        let table = txn
            .open_table(TYPED_TABLE)
            .expect("failed to open typed table");
        let guard = table
            .get("img")
            .expect("failed to get test record")
            .expect("test record not found");
        match guard.value() {
            AbstractData::Image(img) => assert_eq!(img.metadata.ext, "jpg"),
            _ => panic!("expected Image variant"),
        }
    }
}
