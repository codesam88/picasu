use std::path::Path;

use crate::public::constant::{VALID_IMAGE_EXTENSIONS, VALID_VIDEO_EXTENSIONS};

pub fn is_valid_media_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_lowercase)
        .is_some_and(|ext| {
            VALID_IMAGE_EXTENSIONS.contains(&ext.as_str())
                || VALID_VIDEO_EXTENSIONS.contains(&ext.as_str())
        })
}
