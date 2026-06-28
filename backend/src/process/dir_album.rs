use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use anyhow::{Context, Result};
use arrayvec::ArrayString;
use chrono::Utc;
use log::info;

use crate::model::abstract_data::AbstractData;
use crate::model::album::AlbumCombined;
use crate::model::album::AlbumMetadata;
use crate::model::object::{ObjectSchema, ObjectType};
use crate::process::hash::generate_random_hash;
use crate::storage::db::DATA_TABLE;
use crate::storage::db::TREE;
use crate::storage::db::open_data_table;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use redb::ReadableTable;

/// In-memory cache: canonical dir path → album ID.
/// The mutex is held for the full duration of `get_or_create_dir_album` to
/// prevent races under concurrent file indexing.
static DIR_ALBUM_CACHE: LazyLock<Mutex<HashMap<PathBuf, ArrayString<64>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Album IDs that need a self-update after the next in-memory tree refresh.
pub static PENDING_ALBUM_UPDATES: LazyLock<Mutex<HashSet<ArrayString<64>>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Populate `DIR_ALBUM_CACHE` from the database at startup by scanning all
/// albums that have a `dir_path` set.  Must be called after `initialize()`.
///
/// If filesystem albums are enabled, all cached albums are also queued for a
/// stats self-update so their counts are correct from first request.
pub fn init_dir_album_cache() {
    let data_table = open_data_table();
    let mut cache = DIR_ALBUM_CACHE.lock().expect("lock poisoned");

    for entry in data_table
        .iter()
        .expect("failed to iterate table")
        .flatten()
    {
        let (_, guard) = entry;
        if let AbstractData::Album(album) = guard.value()
            && let Some(ref dir) = album.metadata.dir_path
        {
            cache.insert(PathBuf::from(dir), album.metadata.id);
        }
    }

    info!("Loaded {} dir album mappings from database", cache.len());

    if !cache.is_empty() {
        let mut pending = PENDING_ALBUM_UPDATES.lock().expect("lock poisoned");
        for &id in cache.values() {
            pending.insert(id);
        }
    }
}

/// Convert a directory name into a human-readable album title.
/// Replaces `_`, `-`, `.` with spaces and title-cases each word.
pub fn prettify_dir_name(name: &str) -> String {
    name.replace(['_', '-', '.'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Mark `album_id` as needing a statistics self-update.
pub fn mark_album_for_update(album_id: ArrayString<64>) {
    PENDING_ALBUM_UPDATES
        .lock()
        .expect("lock poisoned")
        .insert(album_id);
}

/// Drain and return all albums that need a self-update.
/// Called from `UpdateTreeTask` after the in-memory tree has been refreshed.
pub fn drain_pending_album_updates() -> Vec<ArrayString<64>> {
    PENDING_ALBUM_UPDATES
        .lock()
        .expect("lock poisoned")
        .drain()
        .collect()
}

/// Return the album ID whose `dir_path` is the direct parent of `dir_path`,
/// or `None` if no such album exists in the cache (i.e. `dir_path` is a
/// top-level dir album directly under a sync root).
pub fn get_parent_album_id(dir_path: &Path) -> Option<ArrayString<64>> {
    let parent = dir_path.parent()?;
    DIR_ALBUM_CACHE
        .lock()
        .expect("lock poisoned")
        .get(parent)
        .copied()
}

/// Return the album ID for `dir_path` itself, if it is already a known
/// filesystem-hierarchy album — regardless of whether it was registered via
/// `ensure_dir_albums`'s sync-root walk or some other path (e.g. created
/// directly by a test, or loaded at startup by `init_dir_album_cache`).
pub fn get_album_id_for_dir(dir_path: &Path) -> Option<ArrayString<64>> {
    DIR_ALBUM_CACHE
        .lock()
        .expect("lock poisoned")
        .get(dir_path)
        .copied()
}

/// Return the directory path corresponding to `album_id`, or `None` if it is
/// not a filesystem-hierarchy album (or has not been loaded into the cache yet).
pub fn get_dir_path_for_album(album_id: ArrayString<64>) -> Option<PathBuf> {
    DIR_ALBUM_CACHE
        .lock()
        .expect("lock poisoned")
        .iter()
        .find_map(|(path, &id)| {
            if id == album_id {
                Some(path.clone())
            } else {
                None
            }
        })
}

/// Mark every directory album whose path is a prefix of `file_path` for a
/// stats self-update.  Called from `flush_tree_task` after a media item is
/// written to the database.
pub fn mark_dir_albums_for_path(file_path: &Path) {
    let cache = DIR_ALBUM_CACHE.lock().expect("lock poisoned");
    let mut pending = PENDING_ALBUM_UPDATES.lock().expect("lock poisoned");
    for (dir_path, &album_id) in cache.iter() {
        if file_path.starts_with(dir_path) {
            pending.insert(album_id);
        }
    }
}

/// Find or create the album corresponding to `dir_path`.
/// Must be called from a blocking context (e.g., `tokio::task::spawn_blocking`).
///
/// Holds `DIR_ALBUM_CACHE`'s mutex for the entire duration to guarantee
/// at-most-once album creation per directory under concurrent indexing.
pub fn get_or_create_dir_album(dir_path: PathBuf) -> Result<ArrayString<64>> {
    let mut cache = DIR_ALBUM_CACHE.lock().expect("lock poisoned");

    if let Some(&id) = cache.get(&dir_path) {
        return Ok(id);
    }

    let album_id = write_album_to_db(&dir_path)
        .with_context(|| format!("Failed to create album for {}", dir_path.display()))?;

    cache.insert(dir_path, album_id);

    // Refresh the in-memory tree so the new album is visible immediately.
    BATCH_COORDINATOR.execute_batch_detached(UpdateTreeTask);

    Ok(album_id)
}

// ── internal helpers ───────────────────────────────────────────────────────────

fn write_album_to_db(dir_path: &Path) -> Result<ArrayString<64>> {
    let album_id = generate_random_hash();
    let dir_name = dir_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Album");
    let title = prettify_dir_name(dir_name);
    let dir_path_str = dir_path.to_string_lossy().into_owned();

    let now = Utc::now().timestamp_millis();
    let object = ObjectSchema {
        id: album_id,
        obj_type: ObjectType::Album,
        pending: false,
        thumbhash: None,
        description: None,
        tags: std::collections::HashSet::new(),
        is_favorite: false,
        is_archived: false,
        is_trashed: false,
        update_at: now,
    };
    let metadata = AlbumMetadata {
        id: album_id,
        title: Some(title.clone()),
        created_time: now,
        start_time: None,
        end_time: None,
        last_modified_time: now,
        cover: None,
        item_count: 0,
        item_size: 0,
        share_list: std::collections::HashMap::new(),
        dir_path: Some(dir_path_str),
    };
    let abstract_data = AbstractData::Album(AlbumCombined { object, metadata });

    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for dir album")?;
    {
        let mut table = txn
            .open_table(DATA_TABLE)
            .context("Failed to open data table")?;
        table
            .insert(&*album_id, abstract_data)
            .context("Failed to insert dir album")?;
    }
    txn.commit().context("Failed to commit dir album")?;

    info!(
        "Created filesystem album '{}' (id: {}) for {}",
        title,
        album_id,
        dir_path.display()
    );

    Ok(album_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separators_become_spaces_and_words_are_capitalised() {
        assert_eq!(prettify_dir_name("vacation_2023"), "Vacation 2023");
        assert_eq!(prettify_dir_name("my-holiday.photos"), "My Holiday Photos");
        assert_eq!(prettify_dir_name("road_trip-2022"), "Road Trip 2022");
    }

    #[test]
    fn already_capitalised_words_are_preserved() {
        assert_eq!(prettify_dir_name("Paris"), "Paris");
        assert_eq!(prettify_dir_name("NYC_2024"), "NYC 2024");
    }

    #[test]
    fn consecutive_separators_collapse() {
        assert_eq!(prettify_dir_name("a__b"), "A B");
        assert_eq!(prettify_dir_name("a--b"), "A B");
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(prettify_dir_name(""), "");
    }

    #[test]
    fn unicode_first_char_is_uppercased() {
        assert_eq!(prettify_dir_name("été_photos"), "Été Photos");
    }
}
