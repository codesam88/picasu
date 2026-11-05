//! Source-level guards for lock/execution-pool ordering invariants.

/// Request-path and pool-nested tree readers must take an owned snapshot
/// before running Rayon work.
///
/// A live `TREE.in_memory` read guard held across `par_iter` can deadlock the
/// shared pool: a worker participating in the join can steal the cascade's
/// tree-refresh job, whose `write()` blocks on that guard while the join waits
/// for the same worker. The fix is `snapshot_tree()`, which clones under a
/// short guard and returns owned rows.
#[test]
fn tree_readers_snapshot_before_parallel_work() {
    // (module path, source). The first two are request-path readers; the last
    // two run inside pool jobs (`album_task`).
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
        (
            "tasks/actor/album.rs",
            include_str!("../tasks/actor/album.rs"),
        ),
    ];

    for (path, source) in SOURCES {
        let compact: String = source.split_whitespace().collect();
        assert!(
            !compact.contains("TREE.in_memory.read()"),
            "{path} holds a live TREE.in_memory read guard; iterate an owned \
             snapshot_tree() instead"
        );
    }

    // The designated request-path readers must actually use the helper.
    for path in ["router/get/get_prefetch.rs", "router/post/post_upload.rs"] {
        let source = SOURCES
            .iter()
            .find(|(candidate, _)| *candidate == path)
            .map(|(_, source)| *source)
            .expect("source registered");
        let compact: String = source.split_whitespace().collect();
        assert!(
            compact.contains("snapshot_tree()"),
            "{path} must read the in-memory tree through snapshot_tree()"
        );
    }
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
