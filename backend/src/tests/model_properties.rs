#![cfg(test)]

//! Property-based tests for pure decision logic on the index path.

use proptest::prelude::*;

use crate::model::abstract_data::AbstractData;
use crate::model::asset::canonicalize_path;
use crate::model::image::{ImageCombined, ImageMetadata};
use crate::model::media::is_valid_media_file;
use crate::model::object::{ObjectSchema, ObjectType};
use crate::model::response::FileEntry;
use crate::process::format::kind_for_extension;
use crate::tasks::queue::priority::{LANE_COUNT, SchedulerState, pick_lane};

use arrayvec::ArrayString;
use std::path::PathBuf;

proptest! {
    #[test]
    fn canonicalize_path_is_idempotent(
        rel in "[a-zA-Z0-9_./-]{0,40}",
        home in "/[a-zA-Z0-9_/]{1,20}",
    ) {
        let home = PathBuf::from(home);
        let once = canonicalize_path(&PathBuf::from(&rel), &home);
        let twice = canonicalize_path(&once, &home);
        prop_assert_eq!(once, twice);
    }

    #[test]
    fn canonicalize_path_absolute_ignores_home(
        rel in "/[a-zA-Z0-9_./-]{0,40}",
        home_a in "[a-zA-Z0-9_/]{1,20}",
        home_b in "[a-zA-Z0-9_/]{1,20}",
    ) {
        let a = canonicalize_path(&PathBuf::from(&rel), &PathBuf::from(home_a));
        let b = canonicalize_path(&PathBuf::from(&rel), &PathBuf::from(home_b));
        prop_assert_eq!(a, b);
    }

    #[test]
    fn media_validity_matches_extension_table(
        stem in "[a-zA-Z0-9_]{1,12}",
        ext in "[a-zA-Z]{1,5}",
    ) {
        let path = PathBuf::from(format!("{stem}.{ext}"));
        let expected = kind_for_extension(&ext.to_lowercase()).is_some();
        prop_assert_eq!(is_valid_media_file(&path), expected);
    }

    #[test]
    fn media_validity_false_without_extension(stem in "[a-zA-Z0-9_]{1,12}") {
        let path = PathBuf::from(stem);
        prop_assert!(!is_valid_media_file(&path));
    }

    #[test]
    fn pick_lane_selects_ready_lane(mask in 0u32..(1u32 << LANE_COUNT)) {
        let mut ready = [false; LANE_COUNT];
        for (i, slot) in ready.iter_mut().enumerate() {
            *slot = (mask >> i) & 1 == 1;
        }
        let any_ready = ready.iter().any(|&r| r);
        let mut state = SchedulerState::default();
        match pick_lane(ready, &mut state) {
            Some(idx) => {
                prop_assert!(any_ready, "picked a lane when none ready");
                prop_assert!(ready[idx], "picked lane {idx} was not ready");
            }
            None => prop_assert!(!any_ready, "no lane picked but one is ready"),
        }
    }

    #[test]
    fn pick_lane_none_when_no_ready(_unit in 0..1i32) {
        let mut state = SchedulerState::default();
        let picked = pick_lane([false; LANE_COUNT], &mut state);
        prop_assert!(picked.is_none());
    }
}

fn img_with_scan_time(scan_time: i64) -> AbstractData {
    let id = ArrayString::from("test").expect("array string");
    let mut metadata = ImageMetadata::new(0, 0, 0, "jpg".to_string());
    metadata.path = Some(FileEntry {
        file: "/a.jpg".to_string(),
        modified: 0,
        scan_time,
        is_trashed: false,
    });
    AbstractData::Image(ImageCombined {
        object: ObjectSchema::new(id, ObjectType::Image),
        metadata,
    })
}

fn img_without_path() -> AbstractData {
    let id = ArrayString::from("test").expect("array string");
    AbstractData::Image(ImageCombined {
        object: ObjectSchema::new(id, ObjectType::Image),
        metadata: ImageMetadata::new(0, 0, 0, "jpg".to_string()),
    })
}

proptest! {
    #[test]
    fn scan_time_timestamp_returns_entry_scan_time(scan_time in any::<i64>()) {
        let data = img_with_scan_time(scan_time);
        prop_assert_eq!(data.compute_timestamp(&["scan_time"]), scan_time);
    }

    #[test]
    fn scan_time_timestamp_pathless_returns_zero(_unit in 0..1i32) {
        let data = img_without_path();
        prop_assert_eq!(data.compute_timestamp(&["scan_time"]), 0);
    }
}
