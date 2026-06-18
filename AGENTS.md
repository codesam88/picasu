# Agent Guidelines

## Communication Style

- Stick to facts and established best practice. Do not editorialize or try to be persuasive.
- In assessments and commit messages, reflect uncertainty where it exists. Avoid asserting things as definitive when the evidence is incomplete.
- Skip superlatives and filler ("Clean.", "Perfect.", "This is the right call."). State what happened or what is true and move on.

## Development Workflow

- Take a step back and consider best-practice solutions before diving in.
- Clarify ambiguous requirements with the user before starting.
- Always present the proposed solution and its trade-offs for review before implementing.
- After implementation, give the user a chance to review before moving on.
- Run all available checks and tests before committing: `just precommit` (runs format, check, and test for both frontend and backend); do not rely on the pre-commit hook to catch everything.

## Task Management (.plan)

Every task lives as a markdown file in `.plan/tasks/<slug>.md` with YAML frontmatter. The filename (without `.md`) is the unique key — no numeric ID needed.

### Status lifecycle

| status | meaning | when to use |
|---|---|---|
| `idea` | aspirational, not settled | explore later, not ready to start |
| `backlog` | accepted, deferred | consider when stepping back to plan next work |
| `open` | ready | actionable, waiting to be picked up |
| `in-progress` | active | currently being worked on |
| `blocked` | stuck | note the blocker in the body |
| `done` | complete | finished |

### Agent conventions

- **Discover work:** `cargo xtask plan` lists all tasks; `cargo xtask plan -k` groups by status column. Filter with `-s open`, `-a backend`, `-t bug`, etc.
- **Step back / plan:** `cargo xtask plan -k` to see the full board. Pull items from `backlog` or `idea` when choosing what to work on next.
- **Sort:** flags without values become sort keys — `cargo xtask plan -a -p` sorts by area then priority. `-h` prints all options.
- **Create:** copy `.plan/TEMPLATE.md` to `.plan/tasks/<slug>.md`.
- **Update:** update 'status' to reflect status. append progress notes at the bottom (newest first). Do not rewrite history.
- **Complete:** set `status: done` when finished. Do not delete the file.
- **Block:** set `status: blocked` and note the blocker in the body.
- **Validate:** run `cargo xtask plan --lint` to check; `cargo xtask plan --format` to auto-fix. The precommit hook runs `--format` automatically

## Code documentation

Refrain from including verbose examples and documentation in git commit messages or this AGENTS.md.
Prefer `///` doc comments in the Rust source to document function purpose and any non-obvious design decisions.
Doc comments live next to the code, get updated with it, serve `cargo doc`, IDEs, and agents equally, and never rot.

Items which do not fit into code documentation such as design decisions and usage instructions belong in separate user-focused documentation.

Only document here what cannot be derived from the code itself.

## Non-obvious invariants

- **Album = directory.** Each media item has exactly one album (`metadata.album: Option<ArrayString<64>>`), corresponding to the directory it lives in. Albums are not stored separately — `DIR_ALBUM_CACHE` maps directory paths to album IDs at runtime, rebuilt from the filesystem on every startup. Moving a file to a different album via the API moves the physical file on disk. See `docs/design.md` § Albums.
- **`DIR_ALBUM_CACHE` is a separate `Mutex`.** The album content filter (`generate_filter.rs`) is called while `TREE.in_memory` is already read-locked. Looking up a dir album's path through the tree would deadlock; `DIR_ALBUM_CACHE` avoids this.
- **bitcode encodes struct fields positionally.** Field order in any `Encode`/`Decode` struct is part of the on-disk schema. Adding or reordering fields without a schema version bump silently corrupts existing records.
- **Schema versioning prefix.** Each `AbstractData` record on disk starts with `[0xFF, version]`. `0xFF` is safe because bitcode encodes the 3-variant enum discriminant in bits [1:0] of the first byte (values 0–2); `0xFF` (bits [1:0] = 11) is structurally invalid for the enum and unambiguous as a version marker. Legacy unversioned records (no prefix) are decoded as v1. Schema migration is append-only — never remove a migration wrapper.
- **HTTP error codes must not collide with framework codes.** Rocket returns 404 for unregistered routes. A domain-level "entity not found" must use `ErrorKind::InvalidInput` (→ 400) rather than `ErrorKind::NotFound` (→ 404) to remain distinguishable from routing failures. Reserve `ErrorKind::NotFound` only for cases where a routing-404 is genuinely indistinguishable from the domain error.


# Your skepticism is well-placed. The value of AGENTS.md is often oversold. Let me break it down:
# 
# What an agent can always derive itself (no need to document):
# - File paths, function names, struct fields — grep/find/Read handle these instantly
# - Module structure — tree src/ or cargo modules
# - API surface — cargo doc
# - Recent changes — git log
# 
# What an agent genuinely cannot derive from code:
# - Why a design decision was made
# - Invariants that are enforced by convention not types
# - Gotchas with dependencies (e.g., bitcode encodes positionally — field order matters)
# - The dual-runtime split and why it exists
# 
# That second list is short, stable, and doesn't rot — because if the invariant
# breaks, the system breaks. That's the only content worth putting in AGENTS.md.
# 
# Deterministic options for code indexing:
# - cargo doc --document-private-items — full symbol index, always current
# - rust-analyzer LSP — agents with LSP support (Copilot, Cursor) get this for free
# - cargo modules generate tree — module dependency graph
# - A justfile with documented recipes covering build/test/run workflows
# 
## Practical recommendation:
# 
# Keep AGENTS.md to under ~50 lines covering only the non-derivable invariants
# and gotchas. Invest the saved effort into good /// doc comments in the Rust
# source — they serve humans, IDEs, cargo doc, and agents equally, and they live
# next to the code they describe so they actually get updated.
