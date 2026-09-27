//! The smallest reproducer found for a MIRAI 1.1.12 failure mode that a gate
//! cannot ignore: on ordinary code the tool aborts the compilation with an
//! internal error instead of finishing.
//!
//! `crash_range_loop_accumulator` is a counted loop with an accumulator, which
//! is the shape of every `collect`, `sum` or counter loop in the backend. MIRAI
//! hands the loop's path condition to its Z3 encoding and Z3 rejects the query:
//!
//! ```text
//! Error: Argument (let ((a!1 (and (=> (and (bvslt #x0000000000000000 #x0000000000000001)
//!                          ...
//!   at position 1 has sort (_ BitVec 64) it does not match declaration
//!   (declare-fun bvxor ((_ BitVec 128) (_ BitVec 128)) (_ BitVec 128))
//! ```
//!
//! cargo then reports the compilation as failed, so the exit code is 101, not
//! the 0 that every diagnostic produces. A gate that scrapes `[MIRAI]` lines
//! would read the crash as "no findings" unless it also asserts the exit code.
//! `run-backend.sh` runs this case and asserts exactly that.
//!
//! It lives in its own crate because a crate-wide run of the `../fixture-backend`
//! analysis would abort on it, and one aborting function would make every other
//! row of that matrix unmeasurable.

/// A counted loop with an accumulator. MIRAI 1.1.12 fails on this with an
/// internal error at every diagnostic level, including `default`.
pub fn crash_range_loop_accumulator(timestamp: i64) -> i64 {
    let mut acc = 0i64;
    for i in 0..2i64 {
        acc += i;
    }
    let _ = timestamp;
    acc
}
