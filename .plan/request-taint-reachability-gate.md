---
status: done
type: chore
priority: high
area: testing
---

## Problem

A taint-tracking gate was wanted for the question "can client input reach a
panic construct?" — the failure class behind `GET /get/get-scroll-bar`
panicking on an unknown snapshot id (`.expect("failed to read tree snapshot
for scrollbar")` in `src/storage/cache.rs`, one `spawn_blocking` closure deep)
and behind `get-rows` / `get-scroll-bar` binding `GuardResult<GuardTimestamp>`
and discarding it with `let _ = auth;`. `.plan/rust-taint-gate-evaluation.md`
settled on building the check in-repo on `syn`; this task is that build.

## What was built

- `backend/build/reachability.rs` — pure analysis module (source in,
  findings out, no filesystem, no printing), included by the gate test via the
  same `#[path]` pattern as `build/ast_scan.rs`. Type-based seeds (the
  `Poem`-model reasoning), intra-procedural propagation to a fixpoint over
  name-keyed slots, closures walked inline (incl. `move`, with the receiving
  call as a path frame), interprocedural pushes with per-function
  return summaries, sinks `unwrap`/`expect`/`panic!`/`unreachable!`/`todo!`/
  `unimplemented!`/tainted-index, and a separate guard-propagation check for
  the `let _ = auth;` shape. `ITERATION_LIMIT = 64`; hitting it is an error,
  not a truncation. Limits documented in the module docs — a clean report is
  not a proof of absence.
- `backend/reachability-registry.txt` — 87 entries, key =
  `kind<TAB>file<TAB>function<TAB>subject<TAB>snippet<TAB>reason` (no line
  number: a moved site still matches; a renamed/changed site fails new+stale).
- `backend/src/tests/reachability.rs` — the enforcement point
  (`cargo test --lib`, caught by `just test`): 16 fixture tests (let/method/
  move-closure/spawn_blocking/interprocedural-across-files/Json body/tainted
  index/non-request expect/discarded guard/propagated guard + gate mechanics:
  new site fails, stale entry fails, moved line still matches) plus the
  real-tree gate and a convergence check. `#[ignore]`d
  `dump_reachable_sites` prints full paths on demand.
- `build.rs` does **not** run the analysis: measured ~1.21 s full-crate pass
  (debug, 109 files) is not cheap enough for an advisory warning on every
  build-script rerun; the test is the enforcement point either way.

## Results

- Gate green on arrival: 78 sinks + 9 guards registered, classes
  `false-positive` 84, `pass-through` 3, `request-driven` 0 — no known
  request-to-panic path is reachable in the current tree (the two historical
  bugs are fixed).
- Acid test: reintroducing both historical bugs produced exactly
  `taint-expect src/storage/cache.rs TreeSnapshot::read_scrollbar
get_scroll_bar(timestamp) .expect("failed to read tree snapshot for
scrollbar")` with path `get_scroll_bar(timestamp) ->
TreeSnapshot::read_scrollbar -> .expect(...)` and two `guard-discarded`
  findings (get_rows, get_scroll_bar); the gate failed. Revert verified
  byte-identical to HEAD (`git hash-object` == `git rev-parse HEAD:…` for
  both files).
- `cargo test --lib`: 350 passed (+1 ignored helper). `just check` and
  `just openapi-check` pass; `backend/openapi.json` unchanged.

## Progress

- 2026-09-26: **Done.** Slice delivered as specified (taint + guards + registry
  gate). Observations recorded rather than fixed, per scope: (1) no
  request-driven true positive remains in the tree — every registered site is
  either bounds-guarded (index checks the analysis cannot model), a
  storage-key/data pass-through, or a value-independent sink; (2)
  `AbstractData::compute_timestamp`'s `.expect("failed to convert datetime to
local timezone")` can in principle fire on a DST-ambiguous stored EXIF date
  — a server-side robustness question, not request-driven, reported via the
  registry as pass-through; (3) 9 `_auth: GuardAuth` handlers are flagged by
  the syntactic never-read rule although `FromRequest` enforces those guards
  at extraction — registered as false positives with that reason.
