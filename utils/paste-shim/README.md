# paste shim

Thin wrapper around the maintained `pastey` crate, re-exporting its `paste!`,
`item!`, and `expr!` proc macros under the `paste` crate name.

## Why this exists

The original `paste` crate (dtolnay/paste) is unmaintained (RUSTSEC-2024-0436)
but is pulled in as a transitive dependency by `rav1e` and `little_exif`. This
shim lets us replace it via `[patch.crates-io]` without waiting for upstream
updates.
