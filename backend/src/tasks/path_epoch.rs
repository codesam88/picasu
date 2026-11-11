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
    generations: Vec<(PathBuf, u64)>,
}

impl Drop for PathLease {
    fn drop(&mut self) {
        for (path, generation) in &self.generations {
            release_generation(path, *generation);
        }
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
        let generation = next_generation(&path);
        *EXACT_LEASES.entry(path.clone()).or_insert(0) += 1;
        lease.exact.push(path.clone());
        lease.generations.push((path, generation));
    }
    lease
}

/// Lease a directory subtree for the duration of the returned guard.
pub(crate) fn lease_subtree(path: PathBuf) -> PathLease {
    let generation = next_generation(&path);
    *SUBTREE_LEASES.entry(path.clone()).or_insert(0) += 1;
    PathLease {
        exact: Vec::new(),
        subtrees: vec![path.clone()],
        generations: vec![(path, generation)],
    }
}

/// Lease exact paths and directory subtrees under one guard.
pub(crate) fn lease_all(
    exact: impl IntoIterator<Item = PathBuf>,
    subtrees: impl IntoIterator<Item = PathBuf>,
) -> PathLease {
    let mut lease = lease_paths(exact);
    for path in subtrees {
        let generation = next_generation(&path);
        *SUBTREE_LEASES.entry(path.clone()).or_insert(0) += 1;
        lease.subtrees.push(path.clone());
        lease.generations.push((path, generation));
    }
    lease
}

/// Pure decision rule for "is this scan superseded / should it abort".
///
/// `baseline` is the generation observed when the scan entered `process_file`;
/// `busy` reports a live mutation lease on the path; `current` is the
/// path's generation now. A scan proceeds only when the path is free and
/// the generation is unchanged. A newly submitted path with no baseline
/// (`None -> Some`) also aborts.
pub(crate) fn superseded(baseline: Option<u64>, busy: bool, current: Option<u64>) -> bool {
    busy || current != baseline
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
    fn superseded_truth_table() {
        // Equal generation, free path: proceed.
        assert!(!super::superseded(Some(5), false, Some(5)));
        // Moved generation: abort.
        assert!(super::superseded(Some(5), false, Some(6)));
        // Busy aborts even with equal generation.
        assert!(super::superseded(Some(5), true, Some(5)));
        // None -> Some (new submit, no baseline): abort.
        assert!(super::superseded(None, false, Some(1)));
        // Free, no generation on either side: proceed.
        assert!(!super::superseded(None, false, None));
        // Generation released (Some -> None): abort.
        assert!(super::superseded(Some(5), false, None));
        // Busy aborts regardless of generation.
        assert!(super::superseded(None, true, None));
        assert!(super::superseded(Some(5), true, Some(6)));
    }

    #[test]
    fn lease_releases_generation_on_drop() {
        let path = PathBuf::from("/photos/lease_gen.jpg");
        assert_eq!(current_generation(&path), None);
        {
            let _lease = lease_paths([path.clone()]);
            assert!(current_generation(&path).is_some());
        }
        assert_eq!(current_generation(&path), None);
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
