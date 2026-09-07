---
status: open
type: bug
priority: low
area: backend
---

Logger change premise to verify. Raised in PR \#17 review (2026-09-07).

## Context

`initialize_logger` (`backend/src/init.rs:7-59`) switched from `Builder::new()` + `.init()` to
`Builder::from_default_env()` + `.try_init() .ok()`, and keeps the explicit `.filter(None, log::LevelFilter::Info)`
global fallback (`init.rs:56`) plus a `rocket` module filter (`init.rs:57`).

env\_logger semantics: explicitly added `.filter()` directives take precedence over environment directives for the same
target. Consequence being verified:

1. The explicit global `.filter(None, Info)` overrides RUST\_LOG's root directive, so an operator setting
   `RUST_LOG=warn` (or `error`) cannot lower global verbosity below `Info` — they can only raise module-specific
   levels via more specific directives (e.g. `picasu::x=debug`). If confirmed, this defeats the usual "RUST\_LOG
   controls verbosity" contract and should either use `Env`/`default_filter_or` or drop the explicit global filter.
2. `.try_init().ok()` silently swallows a second `initialize_logger()` call. Acceptable if the function is genuinely
   single-call; confirm no code path can call it twice and expect the second configuration to apply.

## Tasks / decision

- [ ] Confirm precedence claim against the pinned env\_logger version (check `backend/Cargo.lock`) — add a behavior
      test if the codebase has one for logging, or document the nuance on `initialize_logger`.
- [ ] Decide the intended RUST\_LOG contract and align the builder call.
