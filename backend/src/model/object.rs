#![allow(clippy::struct_excessive_bools)]
use arrayvec::ArrayString;
use bitcode::{Decode, Encode};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub enum ObjectType {
    Image,
    Video,
    Album,
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectType::Image => write!(f, "image"),
            ObjectType::Video => write!(f, "video"),
            ObjectType::Album => write!(f, "album"),
        }
    }
}

impl FromStr for ObjectType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "image" => Ok(ObjectType::Image),
            "video" => Ok(ObjectType::Video),
            "album" => Ok(ObjectType::Album),
            _ => Err(format!("Invalid ObjectType: {s}")),
        }
    }
}

/// Common object schema shared between Image, Video, and Album
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct ObjectSchema {
    pub id: ArrayString<64>,
    pub obj_type: ObjectType,
    pub pending: bool,
    pub thumbhash: Option<Vec<u8>>,
    pub description: Option<String>,
    pub tags: HashSet<String>,
    pub rating: Option<u8>,
    pub update_at: i64,
}

impl ObjectSchema {
    pub fn new(id: ArrayString<64>, obj_type: ObjectType) -> Self {
        Self {
            id,
            obj_type,
            pending: false,
            thumbhash: None,
            description: None,
            tags: HashSet::new(),
            rating: None,
            update_at: Utc::now().timestamp_millis(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectSchema, ObjectType};
    use arrayvec::ArrayString;

    fn schema() -> ObjectSchema {
        let id = ArrayString::from("asset-1").expect("failed to create ArrayString");
        ObjectSchema::new(id, ObjectType::Image)
    }

    /// The favorite/archived flags are gone from the object model, so the
    /// serialized shape must not carry them either (the wire JSON of a list
    /// row is derived from this struct).
    #[test]
    fn serialized_object_omits_favorite_and_archived() {
        let value = serde_json::to_value(schema()).expect("ObjectSchema must serialize");
        let map = value
            .as_object()
            .expect("ObjectSchema serializes to an object");

        assert!(
            !map.contains_key("isFavorite"),
            "isFavorite must be removed"
        );
        assert!(
            !map.contains_key("isArchived"),
            "isArchived must be removed"
        );
    }

    #[test]
    fn retained_fields_are_still_present() {
        let value = serde_json::to_value(schema()).expect("ObjectSchema must serialize");
        let map = value
            .as_object()
            .expect("ObjectSchema serializes to an object");

        for key in ["pending", "description", "tags", "rating", "updateAt"] {
            assert!(map.contains_key(key), "retained field {key} must remain");
        }
    }
}
