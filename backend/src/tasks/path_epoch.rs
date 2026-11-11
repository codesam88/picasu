//! Per-path generations and short mutation leases.
//!
//! Queued work carries the generation it was submitted with; a newer submit
//! for the same path wins. A path mutation (API move/delete, directory
//! rename) bumps the paths it touches and holds a lease across its
//! filesystem and row transaction. Background verify jobs capture a
//! generation before checking the path, so a verify that races a mutation
//! either sees a newer generation or an active lease and skips. No
//! process-wide lock is involved.

use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static GENERATIONS: LazyLock<DashMap<PathBuf, u64>> = LazyLock::new(DashMap::new);

static EXACT_LEASES: LazyLock<DashMap<PathBuf, usize>> = LazyLock::new(DashMap::new);

static SUBTREE_LEASES: LazyLock<DashMap<PathBuf, usize>> = LazyLock::new(DashMap::new);

/// Next submission generation for `path`.
pub(crate) fn next_generation(path: &Path) -> u64 {
    let mut slot = GENERATIONS.entry(path.to_path_buf()).or_insert(0);
    *slot += 1;
    *slot
}

/// Mark `path` as touched without enqueueing work.
pub(crate) fn bump(path: &Path) {
    let _ = next_generation(path);
}

/// Latest submitted generation for `path`, if any.
pub(crate) fn current_generation(path: &Path) -> Option<u64> {
    GENERATIONS.get(path).map(|entry| *entry)
}

/// Drop the generation record for `path` when no newer submit replaced it.
pub(crate) fn release_generation(path: &Path, generation: u64) {
    GENERATIONS.remove_if(path, |_, current| *current == generation);
}

/// Held by a mutation for the duration of its filesystem + row transaction.
#[derive(Default)]
pub(crate) struct PathLease {
    exact: Vec<PathBuf>,
    subtrees: Vec<PathBuf>,
}

impl Drop for PathLease {
    fn drop(&mut self) {
        for path in &self.exact {
            decrement(path, &EXACT_LEASES);
        }
        for path in &self.subtrees {
            decrement(path, &SUBTREE_LEASES);
        }
    }
}

fn decrement(path: &Path, leases: &DashMap<PathBuf, usize>) {
    let current = leases.get(path).map(|entry| *entry);
    match current {
        Some(1) | None => {
            leases.remove(path);
        }
        Some(count) => {
            leases.insert(path.to_path_buf(), count - 1);
        }
    }
}

/// Lease exact paths for the duration of the returned guard.
pub(crate) fn lease_paths(paths: impl IntoIterator<Item = PathBuf>) -> PathLease {
    let mut lease = PathLease::default();
    for path in paths {
        if lease.exact.contains(&path) {
            continue;
        }
        *EXACT_LEASES.entry(path.clone()).or_insert(0) += 1;
        lease.exact.push(path);
    }
    lease
}

/// Lease a directory subtree for the duration of the returned guard.
pub(crate) fn lease_subtree(path: PathBuf) -> PathLease {
    *SUBTREE_LEASES.entry(path.clone()).or_insert(0) += 1;
    PathLease {
        exact: Vec::new(),
        subtrees: vec![path],
    }
}

/// Lease exact paths and directory subtrees under one guard.
pub(crate) fn lease_all(
    exact: impl IntoIterator<Item = PathBuf>,
    subtrees: impl IntoIterator<Item = PathBuf>,
) -> PathLease {
    let mut lease = lease_paths(exact);
    for path in subtrees {
        *SUBTREE_LEASES.entry(path.clone()).or_insert(0) += 1;
        lease.subtrees.push(path);
    }
    lease
}

/// Whether `path` has a live mutation. A subtree lease on any ancestor
/// counts.
pub(crate) fn is_busy(path: &Path) -> bool {
    EXACT_LEASES.contains_key(path)
        || SUBTREE_LEASES
            .iter()
            .any(|entry| path.starts_with(entry.key()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_generation_supersedes_older() {
        let path = PathBuf::from("/photos/a.jpg");
        let first = next_generation(&path);
        let second = next_generation(&path);
        assert!(second > first);
        assert_eq!(current_generation(&path), Some(second));
        release_generation(&path, first);
        assert_eq!(current_generation(&path), Some(second));
        release_generation(&path, second);
        assert_eq!(current_generation(&path), None);
    }

    #[test]
    fn lease_makes_exact_path_busy_until_dropped() {
        let path = PathBuf::from("/photos/a.jpg");
        let lease = lease_paths([path.clone()]);
        assert!(is_busy(&path));
        assert!(!is_busy(Path::new("/photos/b.jpg")));
        drop(lease);
        assert!(!is_busy(&path));
    }

    #[test]
    fn subtree_lease_covers_descendants() {
        let root = PathBuf::from("/photos/album");
        let lease = lease_subtree(root.clone());
        assert!(is_busy(&root));
        assert!(is_busy(Path::new("/photos/album/nested/a.jpg")));
        assert!(!is_busy(Path::new("/photos/other/a.jpg")));
        drop(lease);
        assert!(!is_busy(&root));
    }

    #[test]
    fn overlapping_leases_release_independently() {
        let path = PathBuf::from("/photos/a.jpg");
        let first = lease_paths([path.clone()]);
        let second = lease_paths([path.clone()]);
        drop(first);
        assert!(is_busy(&path), "second lease still holds the path");
        drop(second);
        assert!(!is_busy(&path));
    }

    #[test]
    fn duplicate_paths_in_one_call_release_cleanly() {
        let p = PathBuf::from("/photos/dup.jpg");
        let l = lease_paths([p.clone(), p.clone()]);
        drop(l);
        assert!(!is_busy(&p));
    }

    #[test]
    fn repeated_lease_drop_cycles_clear_maps() {
        let p = PathBuf::from("/photos/cycle.jpg");
        let dir = PathBuf::from("/photos/cycledir");
        for _ in 0..3 {
            let l = lease_paths([p.clone()]);
            drop(l);
            let l = lease_subtree(dir.clone());
            drop(l);
            let l = lease_all([p.clone()], [dir.clone()]);
            drop(l);
        }
        assert!(EXACT_LEASES.is_empty(), "EXACT_LEASES not empty");
        assert!(SUBTREE_LEASES.is_empty(), "SUBTREE_LEASES not empty");
    }
}
