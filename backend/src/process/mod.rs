pub mod dir_album;
pub mod exif;
pub mod hash;
pub mod index;
pub mod misc;
pub mod sanitize;
pub mod thumbnail;
pub mod transitor;
pub mod video;
pub mod xmp;
pub mod xmp_write;

use crate::model::album::ResolvedShare;

pub fn resolve_show_download_and_metadata(
    resolved_share_opt: Option<ResolvedShare>,
) -> (bool, bool) {
    resolved_share_opt.map_or((true, true), |resolved_share| {
        (
            resolved_share.share.show_download,
            resolved_share.share.show_metadata,
        )
    })
}
