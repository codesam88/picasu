use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    public::constant::redb::DATA_TABLE,
    public::error::{AppError, ErrorKind, ResultExt},
    public::structure::{abstract_data::AbstractData, album::AlbumCombined, config::APP_CONFIG},
};
use dashmap::DashMap;
use rayon::iter::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};
use redb::{ReadableDatabase, ReadableTable};
use serde::{Deserialize, Serialize};

use super::Tree;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct TagInfo {
    pub tag: String,
    pub number: usize,
}

impl Tree {
    pub fn read_tags(&'static self) -> Vec<TagInfo> {
        // ... (unchanged)
        let tag_counts: DashMap<String, AtomicUsize> = DashMap::new();

        self.in_memory
            .read()
            .unwrap()
            .iter()
            .par_bridge()
            .for_each(|database_timestamp| {
                let abstract_data = &database_timestamp.abstract_data;

                // Count regular tags only
                for tag in abstract_data.tag() {
                    let counter = tag_counts
                        .entry(tag.clone())
                        .or_insert_with(|| AtomicUsize::new(0));
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            });

        let tag_infos: Vec<TagInfo> = tag_counts
            .par_iter()
            .map(|entry| TagInfo {
                tag: entry.key().clone(),
                number: entry.value().load(Ordering::Relaxed),
            })
            .collect();

        tag_infos
    }

    /// Return the albums that are active under the current mode.
    ///
    /// - `album_paths_from_filesystem: true`  → filesystem-hierarchy albums only
    ///   (`dir_path.is_some()`)
    /// - `album_paths_from_filesystem: false` → user-created albums only
    ///   (`dir_path.is_none()`)
    ///
    /// The two sets are kept completely independent: switching between modes
    /// never mutates the other set's records.
    pub fn read_albums(&self) -> Result<Vec<AlbumCombined>, AppError> {
        let dir_mode = APP_CONFIG
            .get()
            .unwrap()
            .read()
            .unwrap()
            .public
            .album_paths_from_filesystem;

        self.in_disk
            .begin_read()
            .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?
            .open_table(DATA_TABLE)
            .or_raise(|| (ErrorKind::Database, "Failed to open DATA_TABLE"))?
            .iter()
            .or_raise(|| {
                (
                    ErrorKind::Database,
                    "Failed to create iterator over DATA_TABLE",
                )
            })?
            .par_bridge()
            .filter_map(|entry| {
                entry
                    .map(|(_, guard)| {
                        let abstract_data = guard.value();
                        match abstract_data {
                            AbstractData::Album(album) => {
                                let is_dir_album = album.metadata.dir_path.is_some();
                                if is_dir_album == dir_mode {
                                    Some(album)
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        }
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()
            .or_raise(|| {
                (
                    ErrorKind::Database,
                    "Failed to collect album records in parallel",
                )
            })
    }
}
