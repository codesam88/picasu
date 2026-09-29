---
status: open
type: feature
priority: high
area: backend
---

## Notes

The reduced specification of `utils/openapi-sanity`: **the checks that require
source analysis and that neither an external OpenAPI document linter nor
`picasu --check-openapi` can perform.** Everything else leaves this tool. Written
2026-09-28 at the user's direction, after `--check-openapi` landed and the
linter-redundancy investigation (`.plan/openapi-contract-hardening.md`, progress
notes for I2 and for the investigation).

### The filter, stated as a test a check has to pass

A check stays only if **both** hold:

1. **It needs facts that exist only in source.** The discriminator is not
   "syn-based" — it is whether the inputs the check compares are absent from
   both other views. The document is the generated `backend/openapi.json`; the
   runtime view is Rocket's mount table from a real `build_rocket()`. A check
   whose inputs are all in the document belongs to a document linter. A check
   whose inputs are all in the mount table belongs to `--check-openapi`.
2. **Nothing in the existing gate already performs it.** As of I2 the gate has
   two document phases, not one: `openapi-artifact` regenerates the document
   from source and diffs it against the committed artifact, and
   `openapi-routes` runs `--check-openapi` against the real mount table. A
   source→document drift is therefore already a failure before the CLI is
   reached; what the CLI adds there is a better error message, not coverage.

The mount table is richer than it first looks, and this decided two rules: a
route's `Route::uri` carries its query bindings and its path segments
(`/get/get-data?<timestamp>&<start>&<end>`, `/get/metadata/<asset_id>`), so
"name-set" agreement about segments and query parameters is _runtime_ data.
`--check-openapi` normalizes the path and drops the query today, so it does not
yet compare them — that half moves there rather than staying in a `syn` tool.

### In scope — the whole specification

Each rule names its source-side facts, its other side, and where it lives now.
The findings keep the source file and line, which is the property a document
linter cannot reproduce.

**A. Guards and who may call an operation** (`src/auth.rs`, `src/guards.rs`)

| #   | rule                                                                                                                                                                 | compares                                                  | why not covered elsewhere                                                                                                                                                                                        |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A1  | the guard classes a handler declares (set equality against its `AUTH_POLICY` entry — missing, extra, or a different class are three distinct messages)               | route attribute's guard types, classified to `GuardClass` | guards are source facts; the document has no guards and Rocket's `Route` does not carry them                                                                                                                     |
| A2  | a `GuardResult<…>` (or wildcard-bound) guard parameter that is never propagated — `let _ = auth;` drops it, so the handler answers `200` where the policy says `401` | guard binding in the function body                        | purely structural; this is the `84f29aa5` `get_rows` class, where annotations and document were both correct                                                                                                     |
| A3  | policy ↔ document pins: an operation in no policy entry, and a policy entry naming an operation the document does not declare                                        | `AUTH_POLICY` against the document's operations           | the policy is Rust data of ours; a linter would need it re-expressed as data. Cheap, and it turns one typo into three useful findings (measured: an `operationId` typo yields the P4 mismatch plus both A3 pins) |
| A4  | documented rejection vs policy: a guarded operation documenting no `401`, a public operation documenting one                                                         | policy entry vs the operation's declared responses        | the expectation comes from our policy, not from the document                                                                                                                                                     |

**B. Request body** (`src/params.rs`)

| #   | rule                                                                                                                                                                     | compares                                               | why not covered elsewhere                                                                                                                                         |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| B1  | body presence: the route attribute's `data = "…"` binding exists exactly when the annotation declares a `request_body` — either direction                                | route attribute vs annotation                          | the handler's body binding is in neither the document nor the mount table; a document linter sees an operation with no body and cannot know a payload is required |
| B2  | body type: the annotation's `request_body = <Type>` names the type the route attribute binds; `request_body = Value` on a typed handler is a finding in either direction | same pair, type identity                               | generation is faithful to the annotation, so neither gate sees a mismatch between the two declarations                                                            |
| B3  | `Form<T>` operations must declare `multipart/form-data`; naming the inner type under another media type is a finding that names the declared media type                  | route attribute's type vs the annotation's media types | the media type's correctness depends on the handler's binding                                                                                                     |

Retain the documented limit on B: a body type no name can be read for (tuple,
slice, array) is skipped rather than reported, and every finding about a form
body names the inner type.

**C. Query-parameter requiredness** (`src/params.rs`)

| #   | rule                                                                                              | compares                                                   | why not covered elsewhere                                                                                          |
| --- | ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| C1  | a document's `required: true` against a handler argument bound as `Option<T>`, in both directions | operation's parameter declaration vs the handler signature | the mount table records `?<name>` but not whether the argument is optional; nothing in the document says it either |

Retain the documented limits on C: not checked when a guard shares the
parameter's name, and not checked when a `FromForm` field fills it (the analyzer
reads signatures, not the types they name).

**D. Optional, decide in this item** (`src/params.rs`)

D1 — `operationId` equal to the handler's function name. Source-linked (a
linter cannot read the handler name) and cheap, but it is a house rule with a
low failure rate; utoipa already derives the id from the function name, so a
finding means a hand-set id. Keep only if a human says a hand-set id is a
defect worth failing the gate over.

### Out of scope — and where each check goes instead

Nothing here is deleted without a named owner. Two owners exist: a phase of
`just openapi-check`, or a document linter to be adopted (open decision 1 of
`.plan/openapi-contract-hardening.md`).

| check leaving                                                                        | owner                                                                                                                                                                            |
| ------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| registered in `routes![]` but carries no `#[utoipa::path]`                           | `openapi-artifact` (regeneration diff) and `openapi-routes` (mounted, undocumented)                                                                                              |
| declared in source but absent from the document, and its inverse                     | `openapi-artifact`                                                                                                                                                               |
| route verb vs annotation verb                                                        | both phases; the gate reports the two sides of the disagreement without naming the handler                                                                                       |
| registered in `routes![]` more than once                                             | nothing — a same-table duplicate collapses to one mount-table entry. Keep only as source hygiene, at the author's discretion                                                     |
| registered but declared in no scanned source file                                    | the tool's own resolution self-check, not a contract check                                                                                                                       |
| path parameter name sets: route segments vs documented placeholders vs `in: path`    | path identity is already compared by `--check-openapi`; the placeholder↔`in: path` half is document-only                                                                         |
| query parameter name sets: route `?<a>` vs `in: query`                               | the data is in `Route::uri`; compare it in `--check-openapi` (a follow-up item, not this tool)                                                                                   |
| a path parameter declared `required: false`                                          | document-only → document linter                                                                                                                                                  |
| tags: no tags, unknown tag, data-API path carrying `pages`, SPA page missing `pages` | document-only → document linter (a tag vocabulary is an `enumeration` rule; the placement rule is one custom function)                                                           |
| duplicate `operationId`, missing `operationId`                                       | document-only → document linter                                                                                                                                                  |
| `$ref` to an undefined component schema; a defined schema nothing references         | document-only → document linter (note: Spectral's CLI hands custom functions a dereferenced document, so its equivalent is unreliable — if this moves, it needs a custom runner) |

**Accepted cost:** route-set drift is now reported by the two gate phases
without the file and line the CLI used to name. Every other finding keeps its
source location. If that locality proves to matter in practice, the cheaper fix
is a better report in `--check-openapi`, not restoring the rule here.

### What the crate keeps

- the reading machinery in full: `routes.rs`, `handlers.rs`, `guards.rs`,
  `modules.rs`, `finding.rs`, the name-level readers in `path.rs`, and
  `main.rs`'s loading and exit codes — the remaining rules still need
  registration→declaration resolution, because a document operation is only
  checkable against a rule once the analyzer knows which handler declares it;
- `auth.rs` (A1–A4) and the requiredness half of P2 and the request-body rules
  of P3 in `params.rs` (B1–B3, C1), plus `main.rs`'s document loading and the
  `--exclude-prefix` filtering, which the surviving document-side rules (A3,
  A4) still need for `/get/test/` and `/assets`.

Deleted with their fixtures and tests: `tags.rs` and `tests/tags.rs` and
`tests/fixtures/untagged/`; the route-set findings of `contract.rs` and the
fixtures that assert them (the file itself shrinks — it also hosts the
registration→declaration resolution the survivors need); the path-parameter and
query name-set findings of `params.rs`, P4a and P5, and their cases in
`tests/params.rs`.

### Sequencing and acceptance

1. Decide D1, and the `--check-openapi` query-binding extension (a separate
   item; this plan records that the half leaves here, not that it has landed).
2. Delete the out-of-scope rules with their fixtures; each deletion cites the
   row above that names its owner.
3. Update `utils/openapi-sanity/README.md` and `docs/openapi-generator.md` with
   the in-scope table and a pointer to `just openapi-check` for the rest, so
   the two gates and the future linter read as one gate with three owners.
4. Add a test asserting the reduced set: a rule that reads only the document is
   the failure mode this item exists to prevent, so the boundary needs a
   mechanical check, not a reviewer's memory. The honest form is a test over
   the crate's rule table (if one is introduced) or a review checklist naming
   each rule's inputs; a regex over the source is not acceptable.

Acceptance: `cargo test -p openapi-sanity`, `cargo clippy -p openapi-sanity
-- -D warnings -A clippy::unwrap_used`, `cargo fmt --check -p openapi-sanity`,
`just openapi-check` and the full `just check` are green; every rule that left
has an owner row above; the README and `docs/openapi-generator.md` state the
boundary; and the surviving suite still proves A2's discarded-guard case and
B/C's drift cases end to end.

### Notes on prior art and interaction

- The route-set half of this tool's work is superseded by I2 of
  `.plan/openapi-contract-hardening.md`; that plan's I1 and I4 as written
  describe CLI work this spec narrows or cancels, and its findings list should
  be read with this item's out-of-scope table.
- The measurement behind the "adopt or not" decision is in the same plan's
  progress notes: 14 of the crate's rules compare source or the mount table
  (kept here), the rest are document-only (leaving).
