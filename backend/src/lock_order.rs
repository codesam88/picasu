//! Runtime lock-order checker (cfg(test)).
//!
//! Under `cfg(test)`, acquisition points for `DIR_ALBUM_CACHE`, the
//! `TREE.in_disk` transactions, and (later) `REBUILD_LOCK` register their
//! resource in a thread-local stack. Acquiring a lower-ordered resource while
//! holding a higher-ordered one panics immediately, turning lock-order
//! inversions into deterministic failures inside every test.
//!
//! In non-test builds the functions are no-ops.

#![allow(clippy::items_after_test_module)]

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Resource {
    DirAlbumCache = 1,
    Tree = 2,
    #[allow(dead_code)]
    RebuildLock = 3,
}

#[cfg(test)]
mod imp {
    use super::Resource;
    use std::cell::RefCell;

    thread_local! {
        static HELD: RefCell<Vec<(Resource, &'static str)>> = const { RefCell::new(Vec::new()) };
    }

    pub fn acquire(res: Resource, site: &'static str) {
        HELD.with(|h| {
            let mut held = h.borrow_mut();
            assert!(
                !held.iter().any(|(r, _)| *r > res),
                "lock-order inversion: acquiring {res:?} at {site} while holding {held:?}"
            );
            held.push((res, site));
        });
    }

    pub fn release(res: Resource) {
        HELD.with(|h| {
            let mut held = h.borrow_mut();
            if let Some(pos) = held.iter().rposition(|(r, _)| *r == res) {
                held.remove(pos);
            }
        });
    }
}

#[cfg(test)]
pub use imp::{acquire, release};

#[cfg(not(test))]
pub fn acquire(_res: Resource, _site: &'static str) {}

#[cfg(not(test))]
pub fn release(_res: Resource) {}
