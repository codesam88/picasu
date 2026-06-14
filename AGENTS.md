# Agent Guidelines

## Development Workflow

- Take a step back and consider best-practice solutions before diving in.
- Clarify ambiguous requirements with the user before starting.
- Always present the proposed solution and its trade-offs for review before implementing.
- After implementation, give the user a chance to review before moving on.
- Run all available checks and tests before committing: `just precommit` (runs format, check, and test for both frontend and backend); do not rely on the pre-commit hook to catch everything.

## Code documentation

Refrain from including verbose examples and documentation in git commit messages or this AGENTS.md.
Prefer `///` doc comments in the Rust source to document function purpose and any non-obvious design decisions.
Doc comments live next to the code, get updated with it, serve `cargo doc`, IDEs, and agents equally, and never rot.

Items which do not fit into code documentation such as design decisions and usage instructions belong in separate user-focused documentation.

Only document here what cannot be derived from the code itself.

## Non-obvious invariants

- **Two independent album systems.** Manual albums store membership in `HashSet` on each media item (`img.metadata.albums`). Filesystem-hierarchy albums compute membership at query time via path-prefix — they never write to `img.metadata.albums`. The discriminator is `AlbumMetadata.dir_path`: `None` = manual, `Some(abs_path)` = filesystem album.
- **`DIR_ALBUM_CACHE` is a separate `Mutex`.** The album content filter (`generate_filter.rs`) is called while `TREE.in_memory` is already read-locked. Looking up a dir album's path through the tree would deadlock; `DIR_ALBUM_CACHE` avoids this.
- **bitcode encodes struct fields positionally.** Field order in any `Encode`/`Decode` struct is part of the on-disk schema. Adding or reordering fields without a schema version bump silently corrupts existing records.
- **Schema versioning prefix.** Each `AbstractData` record on disk starts with `[0xFF, version]`. `0xFF` is safe because bitcode encodes the 3-variant enum discriminant in bits [1:0] of the first byte (values 0–2); `0xFF` (bits [1:0] = 11) is structurally invalid for the enum and unambiguous as a version marker. Legacy unversioned records (no prefix) are decoded as v1.


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
