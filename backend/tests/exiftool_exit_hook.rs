//! The exit hook owns this process's `exiftool -stay_open` children.
//!
//! This lives in an integration test — its own process — because the hook is
//! process-wide: inside the unit suite a process-wide kill would race the
//! parallel tests' sessions. What it exercises is the real read path (a real
//! stay-open child, spawned through `read_metadata_record`) against the real
//! scan and the real kill.
#![cfg(target_os = "linux")]

use picasu::{
    StayOpenExitGuard, exiftool_children_of_this_process, kill_stay_open_children,
    read_metadata_record,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The pinned TIFF fixture — readable EXIF, no sidecar needed.
fn fixture_path() -> PathBuf {
    let entry = snapfab::capabilities::capabilities()
        .fixture_by_id("tiff-48x32-exif")
        .expect("the pinned tiff fixture stays registered in the manifest");
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the backend manifest dir has a parent: the repository root")
        .join(&entry.path)
}

/// New stay-open children this process did not have in `before`.
fn wait_for_new_children(before: &[u32]) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let now: Vec<u32> = exiftool_children_of_this_process()
            .into_iter()
            .filter(|pid| !before.contains(pid))
            .collect();
        if !now.is_empty() {
            return now;
        }
        assert!(
            Instant::now() < deadline,
            "no stay-open child appeared within 10s"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A SIGKILLed child stays in `/proc` as a zombie until its parent reaps it,
/// so "gone" means "not running", not "no such directory".
fn wait_until_gone(pid: u32) {
    for _ in 0..500 {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let state = stat
            .rsplit_once(") ")
            .and_then(|(_, rest)| rest.split_whitespace().next());
        if state.is_none() || state == Some("Z") {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("pid {pid} still running 5s after the kill");
}

#[test]
fn the_exit_hook_finds_and_kills_this_process_stay_open_children() {
    let before = exiftool_children_of_this_process();

    // A real session: one read through the read path starts this thread's
    // stay-open child, and the process-wide scan has to see it.
    read_metadata_record(&fixture_path()).expect("a read starts this thread's session");
    let spawned = wait_for_new_children(&before);
    assert_eq!(
        spawned.len(),
        1,
        "one read starts exactly one child, found {spawned:?}"
    );

    // The guard is what run() holds: dropping it is the exit path.
    drop(StayOpenExitGuard);
    for pid in &spawned {
        wait_until_gone(*pid);
    }

    // The kill function kills too — and the session heals by replacement,
    // the same contract an operator's pkill gets.
    read_metadata_record(&fixture_path()).expect("the session replaces the killed child");
    let second = wait_for_new_children(&before);
    let killed = kill_stay_open_children();
    assert!(
        killed >= 1,
        "the kill reports at least the session child, got {killed}"
    );
    for pid in &second {
        wait_until_gone(*pid);
    }
    read_metadata_record(&fixture_path()).expect("reads recover after the hook kills a session");
}
