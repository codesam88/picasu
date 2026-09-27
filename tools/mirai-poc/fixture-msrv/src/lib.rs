//! The smallest fixture that reproduces the first blocker of the rocket
//! experiment. The crate has no code worth analyzing: `cargo generate-lockfile`
//! alone fails on MIRAI's pinned compiler, because the current versions of
//! `time` and `encoding_rs` declare `rust-version = 1.88` and MIRAI 1.1.12 can
//! only analyze code built by `nightly-2025-01-10` (`rustc 1.86.0-nightly`).
//!
//! `run-shapes.sh` gates both halves: the failure without the pins, and the
//! successful resolve with them.

/// Unused on purpose. See the module comment.
pub fn noop() {}
