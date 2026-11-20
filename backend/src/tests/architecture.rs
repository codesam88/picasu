//! Source-level guards for lock/execution-pool ordering invariants.

/// Request-path and pool-nested tree readers must take an owned snapshot
/// before running Rayon work.
///
/// A live `TREE.in_memory` read guard held across `par_iter` can deadlock the
/// shared pool: a worker participating in the join can steal the cascade's
/// tree-refresh job, whose `write()` blocks on that guard while the join waits
/// for the same worker. The fix is `snapshot_tree_arc()`, which takes an
/// `Arc` handle to the rows under a short guard and returns them owned.
#[test]
fn tree_readers_snapshot_before_parallel_work() {
    // (module path, source). The readers run on request paths or inside
    // pool jobs; every listed file must avoid live guards.
    // `post_upload` stays listed even though it no longer reads the tree at
    // all (store-and-return upload never touches it): the entry pins that.
    const SOURCES: &[(&str, &str)] = &[
        (
            "router/get/get_prefetch.rs",
            include_str!("../router/get/get_prefetch.rs"),
        ),
        (
            "router/post/post_upload.rs",
            include_str!("../router/post/post_upload.rs"),
        ),
        (
            "router/get/get_data.rs",
            include_str!("../router/get/get_data.rs"),
        ),
        ("model/album.rs", include_str!("../model/album.rs")),
    ];

    for (path, source) in SOURCES {
        let compact: String = source.split_whitespace().collect();
        assert!(
            !compact.contains("TREE.in_memory.read()"),
            "{path} holds a live TREE.in_memory read guard; iterate an owned \
             snapshot_tree() instead"
        );
    }

    // The designated request-path reader must actually use the helper, and
    // must use the O(1) `Arc` variant: a per-request row clone is O(rows).
    // `post_upload` is intentionally absent: since the store-and-return
    // rework it never reads the tree (files are stored, indexing is
    // enqueued), so there is no snapshot call to pin — only the no-live-
    // guard invariant above applies to it.
    let source = SOURCES
        .iter()
        .find(|(candidate, _)| *candidate == "router/get/get_prefetch.rs")
        .map(|(_, source)| *source)
        .expect("source registered");
    let compact: String = source.split_whitespace().collect();
    assert!(
        compact.contains("snapshot_tree_arc()"),
        "router/get/get_prefetch.rs must read the in-memory tree through \
         snapshot_tree_arc()"
    );
}

/// Request-path Rayon work runs on the dedicated interactive pool, never the
/// shared bulk pool: `INTERACTIVE_POOL.install` scopes every parallel
/// section below it, and background bulk work can never starve interactive
/// joins no matter how saturated the bulk pool gets.
///
/// This is a textual approximation, not a flow analysis: it pins the scoping
/// calls in place and fails loudly if a request path gains a bare `par_iter`
/// or a direct `rayon::spawn`. True held-guard-across-pool checking is a job
/// for the `picasu-discipline` dylint, which can track guard lifetimes.
#[test]
fn request_paths_run_on_interactive_pool() {
    /// Files that may execute Rayon work on the request path.
    const SOURCES: &[(&str, &str)] = &[
        (
            "router/get/get_prefetch.rs",
            include_str!("../router/get/get_prefetch.rs"),
        ),
        (
            "router/get/get_data.rs",
            include_str!("../router/get/get_data.rs"),
        ),
        ("storage/db.rs", include_str!("../storage/db.rs")),
    ];

    for (path, source) in SOURCES {
        let compact: String = source.split_whitespace().collect();
        assert!(
            compact.contains("INTERACTIVE_POOL.install("),
            "{path} must scope request-path Rayon work with INTERACTIVE_POOL.install"
        );
        assert!(
            !compact.contains("rayon::spawn(") && !compact.contains("rayon::spawn_fifo("),
            "{path} must not spawn raw Rayon jobs; route bulk work through the bridge"
        );
    }
}

/// Source-level guard: no `DIR_ALBUM_CACHE` lock acquisition may occur inside a
/// `TREE.in_disk.begin_write()` ... `commit()` span. The dominant convention is
/// cache mutex BEFORE write txn; inverting it deadlocks against
/// `get_or_create_dir_album`, which holds the mutex across `begin_write`.
fn assert_no_dir_album_call_inside_write_txn(path: &str, source: &str, calls: &[&str]) {
    let mut in_txn = false;
    for (lineno, line) in source.lines().enumerate() {
        if line.contains("begin_write()") {
            in_txn = true;
        }
        if in_txn {
            for call in calls {
                assert!(
                    !line.contains(call),
                    "{path}:{}: `{call}` must not be called inside a write transaction \
                     (snapshot before begin_write)",
                    lineno + 1
                );
            }
        }
        if in_txn && line.contains("commit()") {
            in_txn = false;
        }
    }
}

#[test]
fn transitor_does_not_lock_dir_album_cache_inside_write_txn() {
    let source = include_str!("../process/transitor.rs");
    assert_no_dir_album_call_inside_write_txn(
        "process/transitor.rs",
        source,
        &["ancestor_album_ids_for_path("],
    );
}

#[test]
fn assign_album_does_not_lock_dir_album_cache_inside_write_txn() {
    let source = include_str!("../router/put/assign_album.rs");
    assert_no_dir_album_call_inside_write_txn(
        "router/put/assign_album.rs",
        source,
        &["get_parent_album_id("],
    );
}
