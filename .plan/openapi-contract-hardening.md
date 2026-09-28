---
status: open
type: feature
priority: high
area: backend
---

## Notes

Audit of `docs/openapi-reference.md` (widdershins output) against
`backend/openapi.json`, the utoipa annotations, and the actual Rocket routes.
Route coverage itself is complete (apparent gaps were parser artifacts of
multi-line attributes and Rocket `{x..}` segments); the problems are in the
contract content, organization, and rendering. No work started yet.

## Problem and Hardening Strategy

The exposed API can drift even when a focused rework follows the intended
path-primary design and removes known migration artifacts. The recent review
found stale naming, incorrect identifier values at live call sites, missing
OpenAPI operations, incomplete authentication responses, and documentation
that rendered incorrectly. These defects were not caught by the existing API
or Playwright scenarios because those tests cover selected workflows rather
than the complete public contract. The generated OpenAPI document also did
not prevent drift: route registration, annotations, generated paths, and
public-spec filtering are separate sources of truth.

The goal is one invariant, and every mechanism below serves it: **the generated
OpenAPI content must match what the backend implements at runtime.** The checks
are the means of enforcing it, not the product. The invariant has two halves
that are verifiable by different means, and keeping them distinct is what makes
the enforcement honest:

- **The route set** — every mounted `(method, path)` is in the public spec, and
  every ungated spec operation is mounted. Rocket's route table is the authority
  here, so this half is _provable_ at runtime, not merely reviewed. The
  converse is stated asymmetrically because a build may expose only part of the
  spec: a spec operation absent from a build is expected when it is
  feature-gated and that feature is off, and drift otherwise.
- **Operation detail** — the parameters, bodies, responses and auth on each
  operation. Runtime knows nothing about these; they exist only in the
  `#[utoipa::path]` annotations, so this half is only ever _statically
  derivable_, against the handler source.

The mechanisms below are the means of enforcing those two halves:

1. **Checked-in generated public spec.** Generate the normalized public
   `openapi.json` in CI and compare it with the reviewed repository artifact.
   Any route, parameter, schema, response, security, or documentation change
   must appear in the diff and receive normal code review. (Enforces the static
   half, and keeps the served spec's source-of-record honest.)
2. **Mounted-route/spec parity.** Compare the actual mounted `(method, path)`
   routes with the operations in the public spec. Fail on undocumented routes,
   stale spec operations, duplicate operation IDs, and accidental exposure of
   test-only or internal routes. This is the only mechanism that can _prove_ the
   route-set half, so it runs against the server's real route table rather than inferring one from source. Its exclusions — test-only prefixes and static
   mounts — are backend intent: declared once and read by both the generator and
   this check, never a hand-written string inside a test.
3. **OpenAPI structural linting.** Enforce project rules for operation IDs,
   tags, summaries, descriptions, request schemas, security requirements,
   path/query parameters, and named schemas. This should catch incomplete
   annotations even when an operation is present. **Partial, deliberately
   marked:** among responses only the 401 contract is checked — no rule requires
   a success response, and none checks status families against what a guard can
   produce. Among parameters, names, sets and `required` are checked but types
   are not. So the response and parameter-type halves of this mechanism rest on
   reviewing the artifact diff, not on an enforced rule; I4 and I5 close the
   parameter side, and the status-family rule is a decision still to take.
4. **Breaking-change detection.** Diff the generated spec against the latest
   released baseline and classify removed operations, narrowed schemas,
   newly-required fields, enum changes, response changes, and security changes
   as breaking or review-required. Require an explicit override or release
   note for accepted breaking changes.
5. **Spec-driven contract smoke tests.** Use the OpenAPI document to exercise
   every operation at least for reachability, authentication behavior, input
   validation, unknown-field rejection, expected status families, and response
   schema validation. Start with deterministic seeded fixtures and expand to
   property-based testing only where the endpoint state model permits it.

## Tasks

High — contract wrong or materially incomplete:

- [x] Register `POST /post/renew-hash-token` in `openapi.rs` — it has an
      utoipa annotation (`auth.rs`) but was never added to the paths list, so
      it is absent from the spec; sibling `/post/renew-timestamp-token` is
      documented (asymmetric omission). Add a coverage test comparing mounted
      routes against the spec so this class of omission cannot recur.
      **Closed as premise-not-reproducible (2026-09-28)** — the handler is
      annotated (`auth.rs:615`), mounted (`auth.rs:174`), and listed in the
      build.rs-generated paths (`openapi.rs:66,142`); `backend/openapi.json`
      has contained `"/post/renew-hash-token"` since the artifact was first
      committed (`b679091a`). At writing time (2026-09-23) the spec was
      gitignored and `backend/src/openapi.rs` did not exist (build.rs
      generates it), so the claim could only have come from a stale artifact
      — most plausibly the known-stale `docs/openapi-reference.md`, which had
      no occurrence. The coverage test it asked for exists: `check_contract`'s
      "declared in source but absent from the spec" finding and
      `backend/src/tests/openapi_contract.rs`.
- [x] Document `401` on guarded operations: only 2/65 ops currently declare
      `401` (`authenticate`, `/unauthorized`) while data endpoints sit behind
      `GuardAuth`/`GuardTimestamp` (24 router files). Introduce a reusable
      `Unauthorized` response component and sweep it across `#[utoipa::path]`
      responses (or post-process the generated spec). **Done** — one
      `Unauthorized` `ToResponse` component in `backend/src/openapi_components.rs`,
      registered by the `build.rs` generator from a single `RESPONSE_COMPONENTS`
      list, referenced by 39 operations (2 → 40 declaring a 401: +37 in the
      sweep, +2 when `get-rows`/`get-scroll-bar` were fixed). Each route was
      checked individually: `GuardReadOnlyMode` answers 405;
      `get-rows`/`get-scroll-bar` originally discarded their guard result —
      that gap was fixed (guard propagated, both operations now declare 401)
      and recorded in `bug-get-rows-auth-guard-discarded.md` (done).
      `POST /post/authenticate` lost its specific "Invalid
      password" wording because utoipa cannot attach a description to a `$ref`;
      the shared description covers it. Follow-up: 405 is declared on only one of
      the 20 `GuardReadOnlyMode` routes.
- [x] Remove test-only probes from the public spec: `probe_record`,
      `probe_dupe_group` + `TestRecordProbe`/`DupeGroupMember` schemas are
      registered unconditionally; handlers compile into production and are only
      flag-gated (404 unless test bootstrap). **Done** — `--dump-openapi` now
      serves `openapi_public::public_json()`, which strips `/get/test/*` and
      the two probe schemas; probe contract tests keep the full generated spec.

Medium — bad patterns and type fidelity:

- [x] Tag every operation: 39/65 ops have no tags, the other 26 all carry the
      misnomer `pages`; reference grouping/TOC is effectively random. Suggest
      `auth`, `albums`, `assets`, `config`, `index`, `metadata`, `shares`,
      `serving`, `upload`. **Done** — all 61 public ops now carry exactly one
      tag; distribution: `pages` 22, `albums` 9, `assets` 7, `config` 6,
      `timeline` 6, `index` 5, `auth` 3, `serving` 2, `upload` 1. The
      suggestion list needed two corrections: `metadata` and `shares` were not
      used (their operations belong to `assets` and `albums` respectively),
      and `timeline` — the grid-data group (`prefetch`, `get-data`,
      `get-rows`, `get-scroll-bar`, `get-tags`, `get-export`) — was missing
      from it. Gated by `every_operation_carries_a_known_tag` in
      `backend/src/tests/openapi_contract.rs` (known set, missing tags, and
      `pages`-by-path-shape), with negative self-checks; taxonomy documented
      in `docs/openapi-generator.md`.
- [ ] Add missing schema field descriptions: ~205 table rows in the reference
      still end in `none` and 36 of 45 schemas have at least one undescribed
      property (`EditTagsData`, `DeleteList`, `CreateShare`, `Prefetch`, …).
      Existing hand-written `///` comments (`FileEntry`, `coverHash`) are the
      quality bar.
- [ ] Fix nullable/union rendering: the artifact now carries an explicit
      `oneOf: [null, …]` for the cited `PrefetchReturn.resolvedShareOpt`, but
      the generated reference still degrades it — the type column renders
      `any` and the example synthesizes `{}`. Keep the type through tables and
      examples.
- [ ] Fix `FsCompletion` wire casing: serializes `is_default` (snake) while
      every other schema is camelCase — missing
      `#[serde(rename_all = "camelCase")]`. Real API inconsistency, not just
      docs.
- [ ] Fix mangled headings: six operations carry multi-line utoipa summaries
      (`/get/metadata/{asset_id}`, `/object/imported/{file_path}`,
      `/post/create_dir_album`, `/post/index/album`, `/post/index/image`,
      `/{path}`), and the newline ends the generated heading early — e.g. the
      reference renders `## Walk a directory under … in the` and drops
      `background.` into body text (lines 8122, 8429, 3053), breaking
      Parameters/Responses in-page links and duplicating
      description-as-anchor. Keep utoipa summaries single-line titles.
- [ ] Revisit typed response schemas and `FileEntry`. `FileEntry` is registered
      as a component schema but referenced by no operation, so the Step 4
      orphan rule unregisters it; its doc comment says it is meant for the
      wire through `metadata.path`. Decide in a dedicated review which
      operations should expose typed response schemas and whether `FileEntry`
      returns as a `$ref`. Raised 2026-09-27, deferred pending that review.
      Measured 2026-09-28: 14 of 61 operations declare a success response naming
      a schema, 4 declare an inline `text/plain` string, and the rest declare no
      body. The two handlers that return a typed payload with no documented
      schema are `get_metadata` (`Json<AbstractData>`) and `authenticate`
      (`Json<String>`) — the first is the largest data response in the API and
      the reason `FileEntry` is orphaned, since no operation names
      `AbstractData` at all.
- [x] Update `docs/openapi-generator.md`: 4 references to
      `docs/mdbook/src/openapi-reference.md` are stale; `justfile` writes
      `docs/openapi-reference.md`. Anything wiring a CI drift-check against
      the doc path checks a nonexistent file. **Done** — paths corrected, the
      nonexistent `just openapi-docs`/`openapi-docs-check` recipes replaced with
      the real `just docs-openapi`/`just openapi-check`, and the checked-in
      artifact plus the parity gate documented.

Low — consistency and polish:

- [x] Document remaining `/upload` query params on the utoipa path: only `auto_rename` is annotated; add
      `presigned_album_id_opt` and `on_conflict` (valid values `skip`|`rename`, defaults). Absorbed from
      `openapi-upload-query-params.md` (2026-09-24). **Done** — both are annotated with
      descriptions (Step 4's parameter gate reported them, and the fix landed with the
      other 25 findings in the same change).
- [ ] Add a description to the bare `AlbumIndexState` enum (state meanings:
      `idle`/`running`/`completed`/`canceled`/`failed`). `OnConflict` and
      `AssignOutcome` already carry value semantics.
- [ ] Decide a naming convention and document it: snake query params
      (`on_conflict`, `auto_rename`) vs camel bodies (`onConflict`) vs mixed
      path styles with verb-doubling (`/get/get-data`). Path changes are
      breaking — documenting the rule is enough for now.
- [ ] Decide static-route policy: `/assets/<file..>` is undocumented while
      the SPA catch-all `/{path}` is documented; pick neither or both.
- [ ] Rename `DataBaseTimestampReturn` (typo-cased legacy name).
- [ ] Optional bloat pass: 65 ops × 9 sample languages ≈ 70% of the 11k-line
      file, including 39 widdershins "backwards compatibility" boilerplate
      hits; consider `widdershins --summary` or trimming sample languages.

## Shared OpenAPI Sanity Work

This is the implementation plan for the first two mechanisms above, not a
separate task. The existing `build.rs` route generator, OpenAPI contract tests
and generated-artifact check should converge on one reusable source/spec
analyzer while keeping runtime checks and endpoint behavior tests separate.

### Step 1: Extract the source analyzer

- Create a workspace crate at `utils/openapi-sanity/` containing the current
  `syn` route discovery and canonical Rocket-to-OpenAPI path translation.
- Make `build.rs`, backend contract tests and the CLI consume the same library;
  remove the current `#[path]` duplication after the consumers are migrated.
- Preserve byte-identical `backend/src/openapi.rs` and
  `backend/openapi.json` output.
- Keep malformed-source diagnostics deterministic and non-panicking.

### Step 2: Make `openapi-check` the unified contract gate

- Add a deterministic CLI semantic check for source/spec consistency:
  missing function-local annotations, path/method mismatches, source/spec
  operation drift, duplicate handler identities and duplicate operation IDs.
- Make `just openapi-check` run the semantic check and then the generated
  artifact diff. `just check` and CI require only this command.
- Keep runtime mounted-route parity in backend integration tests; source
  analysis cannot replace feature-aware runtime inspection.
- Verify each failure mode with an injected mutation and restore it to green.

### Step 3: Align authentication policy with implementation

- Derive observed guard types from handler parameters, distinguishing direct
  guards from deferred `GuardResult` guards.
- Require protected operations to have an approved guard and matching OpenAPI
  `401`/security policy.
- Require deferred guard results to be propagated or inspected.
- Keep public pages and intentional unauthenticated entry points as explicit
  policy exceptions. Do not infer security from subject tags.
- Migrate the existing tag and shared-401 test policy only after the new
  library/CLI produces equivalent diagnostics.

### Step 4: Add parameter and documentation consistency selectively

Only add checks that have actionable source information:

- OpenAPI path parameters match handler parameters;
- optional query parameters match `Option<T>` shapes;
- request-body declarations match `Json<T>` and upload inputs;
- operation IDs remain stable across reviewed spec changes;
- response schemas are referenced and not orphaned.

Do not turn this into a second OpenAPI serializer or a general Rust taint
analyzer. Request-to-panic analysis remains a separate reachability task.

#### Design decision (2026-09-27): checker, not generation

Reviewed and rejected auto-generating the structural half of `#[utoipa::path]`
from the route attribute and signature (attribute macro, or build-time
derivation merged at dump time): annotations keep prose regardless, the macro
option couples to Rocket's expansion order and utoipa's argument grammar, and
the plan's boundary favours verification. Step 4 is therefore a fifth rule
group in the CLI, `check_params`, chained after `check_contract`,
`check_tags` and `check_auth`:

- **P1 — path parameters.** The placeholders in the spec path, the route's
  `<segment>` names after `to_spec_path` normalization (`<_path..>` → `path`),
  and the operation's declared `in: path` parameters must be the same set —
  no undocumented segment, no declared parameter the route does not bind.
- **P2 — query parameters.** The route's `?<a>&<b>` names and the operation's
  declared `in: query` parameters must be the same set, and each parameter's
  `required` flag must agree with `Option<T>` on the bound handler argument.
- **P3 — request body.** A `data = "<x>"` argument exists iff the operation
  declares `requestBody`; the declared schema must name the handler's data
  type after unwrapping `Json<T>`/`Form<T>`. `request_body = Value` is a
  finding when the handler takes a typed body; multipart inputs are satisfied
  by a `multipart/form-data` content type.
- **P4 — operation ids.** The spec's `operationId` must equal the handler
  function name (utoipa's default; no annotation sets one today). Stability
  across reviewed changes is mechanism 4's job, not this rule's.
- **P5 — schemas.** No `$ref` to an undefined component schema, and no defined
  schema that nothing references.

The rules are expected to fire on the repository as it stands (10 operations
without path parameters, 11 query parameters undocumented, `Value` bodies on
typed handlers, `FileEntry` orphaned); those findings are fixed in the same
change so the gate stays green.

### Step 5: Review findings to investigate

A review of the branch on 2026-09-28 probed the gate for drift it would not
report, and the artifact and the source for claims that no longer hold. The
evidence recorded is what the probe saw, and each item was a pointer to confirm
against the code before acting on it. The items were recorded before the design
was settled, so several have since been decided: each finding below now carries a
pointer to the implementation item that owns it, or states that it is unowned.
Findings marked **→ I1**–**→ I5** are covered; **Partly done** is covered in part;
**Unowned** is a design decision the invariant does not require, still open.

The findings fall into two tiers, and the split matters for what is worth doing
first. The route-set findings ask whether the gate can _see_ the routes at all — a
gate that cannot see a route enforces nothing, so they are the load-bearing ones.
The operation-detail findings ask whether the checks it does run are _sound_; two
of them give a wrong answer where a skipped check would be honest, which is the
failure mode that erodes trust in a gate.

#### Implementation plan

The items in this step reduce to three pieces of work, in this order. Each one is
a prerequisite for the next: the walk makes the generated spec complete, the
complete spec is what the parity check has something to compare against, and the
exclusion policy is what both sides read.

**I1 — Complete the generated spec by walking the backend source (retire
`SCANNED_MODULES`).** The crate already does the hard part: `scan_routes` and
`scan_handlers` take a `syn::File` and know nothing about which file it is. Only
the _file selection_ is a hardcoded list today. So:

- Add a crate-level walk, e.g. `discover_sources(app_root) -> Vec<SourceUnit>`,
  that recurses every `.rs` file under `backend/src`, skipping the `tests/` tree
  and any `#[cfg(test)]` item. The `syn` visitor already ignores doc comments, so
  the `routes![]` occurrences under `src/tests/` are not a hazard; skipping
  `tests/` is belt-and-braces.
- Generalize `group_and_module` (`modules.rs:93`) from its one-directory-level
  `split_once('/')` to arbitrary module depth, and re-key `SourceUnit` and
  `handler_module_path` off the **application-root-relative** path instead of the
  router-relative one, so a `routes![]` outside `router/` resolves too. This is
  the only genuinely new parsing logic; the resolution rule has never been
  exercised past one level because no nested `routes![a::b::c]` exists today, so
  it needs its own fixture.
- Point `build.rs` (`collect_all_routes`, currently iterating `SCANNED_MODULES`)
  and the CLI (`main.rs`) at the shared walk, and delete the constant.
- Delete the `backend/src/tests/route_scan.rs` guards that exist only to police
  the list, including `scanned_router_modules_cover_every_mounted_group` (its
  fixed five names detect removals, not additions — the walk is what closes that).
- Replace the source-text assertion `build_script_scans_with_the_shared_analyzer`
  with a behavioural one: generate a spec from a fixture tree through the same
  entry point `build.rs` uses.
- Retire the `--module` and `--router-root` CLI flags (the walk replaces file
  selection) and update the surfaces that name them: the `justfile`
  `openapi-sanity` recipe (still passes `--router-root`), the crate README's
  option table and `SCANNED_MODULES` reference, and `docs/openapi-generator.md`.
  The doc is written to the post-walk shape; the code flags change with I1.

Acceptance: a `routes![]` block in a file no one listed — a new
`src/api/v2.rs`, say — is picked up by both the generator and the CLI, and a
feature-gated registration is either reported or explicitly excluded per I3.

**I2 — Prove route-set parity behind `--check-openapi`.** A new mode on the
binary, a sibling of `--dump-openapi`:

- `main.rs` gains `--check-openapi <path-to-spec>` (defaulting to the committed
  `backend/openapi.json`). Unlike `--dump-openapi`, it reads the **committed**
  spec from disk rather than the compiled-in one — that is the published claim,
  and reading it is what makes the check a gate on the artifact.
- Build the real `build_rocket()` (not `build_test_rocket()`), read `.routes()`,
  normalize both sides with the shared `to_spec_path`, and apply the asymmetric
  rule: every mounted route must be in the spec (hard fail); a spec operation not
  mounted in this build is excused only if it is feature-gated and that feature is
  disabled here; an ungated spec-only operation is drift. Read this build's
  enabled features via `cfg!(feature = ...)`.
- Exit non-zero on drift with a per-route report; the spec-dependency stays in the
  repo, not in the server's boot.
- Run it in the release gate and the commit hook, pinned to the shipped feature
  set (`--features embed-frontend`), so the build checking is the build shipping.
- Add a fixture-tree negative test for the drift case, then retire the two
  `openapi_contract.rs` parity tests to self-checks; the load-bearing assertion
  moves here.

**I3 — Express exclusions as one policy both sides read.** The `is_outside_contract`
strings in `openapi_contract.rs:61` and the `--exclude-prefix` argument in the
`justfile` both name the same exceptions. Put them in one backend-owned place (a
const or a small policy file) that the generator, the CLI invocation, and the
`--check-openapi` comparison all consume, so the generator and the gate cannot
disagree about what is in the contract. Exclusions remain for genuinely non-API
surfaces only — static file mounts, test-only probes — not for feature-gated APIs,
which I1/I2's asymmetric rule handles without one.

**I4 — Compare parameter and body types, not just names.** P1–P3 check names,
sets and `required`, so a query parameter declared `type: string` against a
handler taking `i64` passes, and `PRIMITIVE_TYPES` already maps JSON primitives
to Rust spellings for bodies. Do this one deliberately rather than by adding to
the table:

- Read the override representation first. This repository sets
  `#[schema(value_type = String)]` on enum variants, so a declared type and the
  Rust type legitimately differ; a rule that ignores the override
  false-positives on every such field. Establish the actual set of overrides in
  use and decide the comparison from it, rather than assuming the mapping is
  identity-plus-primitives.
- Decide what an unreadable type means for each direction, and be explicit that
  "cannot read" is not "matches" — the same reasoning as I5, applied to types.
- Fixtures for the divergence cases, not only the matching ones: a
  `value_type` override, a `Vec<T>`, an `Option<T>`, and a bare primitive.

Sizing note: this is design work, not a lookup-table addition, and the override
set should be measured before the rule is written. If the override surface turns
out to be broader than the enum-variant case, narrowing to a documented
supported subset beats a rule that is right about most routes and noisy on the
rest.

**I5 — Fix the two analyzer rules that give a wrong answer instead of no
answer.** Both currently misreport rather than staying silent, which is the
worse failure: a check that cries wolf is a check people learn to skip.

- `body_drift` (`utils/openapi-sanity/src/params.rs`) substitutes the string
  `a type this analyzer cannot name` for an unnamed argument and then compares it
  like a name, so it can only ever mismatch. `body_type` in
  `handlers.rs` returns no name for a tuple, a slice or a reference. Make the body
  rule skip what it cannot read, as the query rule already does. Confirm with a
  fixture first.
- The `required` rule is vacuous for a parameter it cannot bind to a plain
  argument: `plain_argument` matches `ArgKind::Plain` by name, so a struct-bound
  parameter, or a guard sharing the parameter's name, gets neither check nor note.
  Confirm whether any current route is in that shape, then either resolve the
  struct's fields or state the limit in the rule's documentation. Silently not
  checking is the part to fix either way.

Ordering: I1 → I2 → I3, then I4 and I5. I2 depends on I1 (a complete spec is what
"all production routes are documented" is measured against). I3 is independent but
should land with I2, since both read the same policy. I4 and I5 touch only
`params.rs` and `handlers.rs` — not discovery, not the runtime check — so they
depend on none of I1–I3 and can be done in parallel with that chain rather than
queued behind it.

#### Ordered by the invariant, not by severity

The findings are grouped by which half of the invariant they threaten and by what
closes it, so work on the provable half is not blocked behind refinements of the
static half.

#### The gate's shape, settled 2026-09-28

Two derivations of the route table, deliberately kept independent:

1. **Static, by `syn`.** `scan_routes` and `scan_handlers` walk the source AST
   and find `routes![...]` registrations and `#[utoipa::path]` annotations. This
   is what the CLI runs, and it needs no compiled artifact, which is what lets it
   run in the pre-commit hook and in `just check`.
2. **Runtime, from Rocket.** The route table of the real `build_rocket()`,
   built with the shipped feature set, reflects what the server actually serves,
   including a `FileServer` mount that has no `routes![...]` to find. This is
   `--check-openapi`'s input; it is deliberately not the
   `build_test_rocket()` in the backend test, because a test build runs without
   `embed-frontend` and therefore serves a different table than the one that
   ships.

The CLI does not take a route inventory from the server, and neither derivation
becomes the other's authority: they are different methods, and their agreement is
the evidence. What is wrong today is not the method but the file list — the
static side parses only `SCANNED_MODULES`, so its coverage is asserted rather
than derived. The implementation plan above fixes that, and the findings below
are the same work seen from the review's angle.

**Route set — provable at runtime.** These close the half of the invariant that
can actually be proven, and the first is the precondition for all of them: the
served spec has to be _complete_ before parity can mean anything.

- [ ] Make the generated spec complete: walk the application root. Only the
      five modules in `SCANNED_MODULES` are parsed, so a `routes![...]` block
      anywhere else is invisible to the source gate. A probe tree whose sixth
      module mounted an annotated but undocumented `/secret?<token>` handler
      produced no finding about it. The gap is the file list, not the analysis:
      `scan_routes` is a `syn` visitor that matches `routes!` by its last path
      segment and walks function bodies, so it would report `builder.rs:135` on
      the first file it was handed. Walk the application root and parse every
      file with the visitors that already exist — skip the `tests/` tree and
      `#[cfg(test)]` items, or the gate reads its own test suite as API (all four
      `routes![]` occurrences under `backend/src/tests/` are in doc comments).
      Scope the walk to the backend source, not `router/`: a route table outside
      `router/` is equally invisible today. **→ I1.**
- [ ] Retire `SCANNED_MODULES` once the walk exists. It conflates two questions —
      which files exist, which is derivable by walking, and what is part of the
      contract, which is a decision. The decision survives as a path-based
      exclusion (`--exclude-prefix`, and whatever the static-route policy makes
      of `/assets`) read by both the build script and the CLI. Removing it also
      removes the two guards in `backend/src/tests/route_scan.rs` that exist only
      to police it, including `scanned_router_modules_cover_every_mounted_group`,
      which hardcodes the same five names and so detects removals and not
      additions. The walk belongs in the crate as one function, shared by
      `build.rs` and the CLI, so the generator and the gate cannot disagree about
      what the API's files are. **→ I1.**
- [ ] Express exclusions as declared backend intent, read by both sides. Settle
      what a route discovered only by the source scan means for a feature-gated
      or `cfg(test)` mount. `builder.rs:135` is
      `#[cfg(feature = "embed-frontend")]` and the default build mounts a
      `FileServer` at the same path, which has no `routes![]` at all — so the
      source scan and the runtime comparison see different things for `/assets`
      by construction, and any exclusion has to express "behind a feature" and
      "not a route table" rather than a file path. Today this invariant is
      _manually overridden_ by hand-written strings in `is_outside_contract`; the
      walk turns that into a first-class, shared policy rather than a test-local
      string. **→ I3.**
- [ ] Feature-gated routes: one canonical spec, feature-dependent operations
      marked. A single `openapi.json` describes every route any build can expose —
      the union across features, not one build's slice — and an operation that
      exists only under a feature carries that feature as a vendor extension
      (`x-picasu-feature: embed-frontend`). utoipa 5.5 supports
      `extensions(...)` on `#[utoipa::path]` (utoipa-gen-5.5.0/src/lib.rs:1042),
      so the marker is a supported mechanism, not a hand-patched field. This
      makes the product-build parity check asymmetric, which is what
      feature-gating requires: every route the running product actually mounts
      must be in the spec (hard failure), but a spec operation that is not
      mounted in _this_ build is acceptable when — and only when — it is
      feature-gated and that feature is disabled here. An ungated spec operation
      with no matching mount is still drift. The symmetric
      `every_spec_operation_is_mounted` test cannot express this, which is why
      feature-gating is awkward under it today and why `/assets` needed a
      hand-written exclusion; the asymmetry removes the need for that exclusion
      for feature-gated APIs, leaving exclusions only for genuinely
      non-API surfaces (static file mounts, test-only probes). **→ I1, I2.**
- [ ] Prove route-set parity behind a `--check-openapi` flag, in the product
      build. The only mechanism that can prove the route-set half is Rocket's real
      mount table, and only a real build has it correctly: `build_rocket()` is
      reached on the launch path, and only the shipped feature set
      (`--features embed-frontend`) carries the feature-gated registrations. A
      test cannot stand in for this — `just test` builds without `embed-frontend`,
      so a route registration behind a feature is absent from the test table and
      the check can neither flag nor cover it, and it only runs when someone runs
      `cargo test`. Add a `--check-openapi` mode to the binary (a sibling of
      `--dump-openapi` in `main.rs`, but the check rather than the generator): read
      the committed `backend/openapi.json` — the published claim — build the real
      `build_rocket()`, read `.routes()` (cheap; neither ignites nor launches), and
      compare the two under the asymmetric rule above. Fail with a precise message
      on drift. The spec dependency is deliberately confined to where the spec is
      authored and shipped — the repo, the commit hook, CI and the release gate —
      not the server's runtime: `openapi.json` is a review artifact, not a
      deployment dependency, so boot must not depend on it and startup must stay
      clean. Run the flag unconditionally in the release gate so every shipped
      artifact is verified against its own claim, with a fixture-tree negative
      test for the drift case.
      This also collapses the `SCANNED_MODULES` blind spot: a `routes![]` in a file
      the generator never scans never reaches the spec, so it appears as a
      mounted-but-undocumented route and fails the check rather than passing
      silently. Assert inside the binary rather than diffing two JSON blobs in the
      shell — it has both sides in memory, normalizes route paths the way the
      runtime test does, and can share the exclusion policy. The route table
      (Rocket's live mount) and the spec (utoipa's compiled annotations, committed)
      stay derived from genuinely different sources; comparing them is the proof,
      and deriving them from one another would make them agree by construction and
      stop being evidence. Once this lands, the two `openapi_contract.rs` parity
      tests retire down to their negative self-checks — the load-bearing assertion
      moves out of the test harness. **→ I2.**

**Operation detail — static only.** Runtime knows nothing about these; they exist
only in the annotations, so they can only be checked against the handler source.
The walk above is a precondition here too — a route in an unscanned file has no
checked detail at all.

- [ ] A body type the analyzer cannot name is reported as drift instead of
      skipped. `body_drift` (`utils/openapi-sanity/src/params.rs`) substitutes
      the string `a type this analyzer cannot name` for an unnamed argument and
      compares it like a name, so it can only mismatch; `body_type` in
      `utils/openapi-sanity/src/handlers.rs` returns no name for a tuple, a slice
      or a reference. The query rule skips the same situation deliberately.
      Confirm with a fixture, then make the body rule skip what it cannot read.
      **→ I5.**
- [ ] Parameter types are never compared. P1–P3 check names, sets and `required`,
      so a query parameter declared `type: string` while the handler takes `i64`
      passes, and `PRIMITIVE_TYPES` already maps JSON primitives to Rust
      spellings for bodies. Read the override representation first: this
      repository sets `#[schema(value_type = String)]` on enum variants, so a
      declared type and the Rust type legitimately differ and a naive rule would
      false-positive on every such field. **→ I4.**
- [ ] Mechanism 3 is partial and the strategy list does not say so. Only the 401
      contract is checked among responses: no rule requires an operation to
      declare a success response, or checks status families against what a guard
      can produce — the 20 `GuardReadOnlyMode` routes answer 405 and, as the 401
      task records, one of them declares it. Mark the mechanism partial in the
      strategy section, then decide whether a status-family rule belongs in this
      tool or in the backend contract test. **Partly done** — the strategy section
      now states the mechanism's limits; the status-family rule is still undecided.
- [ ] The `required` rule is vacuous for a parameter it cannot bind to a plain
      argument: `plain_argument` matches `ArgKind::Plain` by name, so a
      struct-bound parameter, or a guard that shares the parameter's name, gets
      no check and no note. Confirm whether any current route is in that shape,
      then resolve the struct's fields or state the limit in the rule's docs.
      **→ I5.**
- [ ] `build_script_scans_with_the_shared_analyzer`
      (`backend/src/tests/route_scan.rs`) asserts that `build.rs` contains the
      strings `openapi_sanity::scan_routes` and `openapi_sanity::scan_handlers`.
      It fails on a spelling change that keeps the shared analyzer, and passes on
      a build script that adds a second, private scanner beside it. Replace it
      with a behavioural assertion — generating a spec from a fixture tree
      through the entry point `build.rs` uses is the cheapest. Low effort.
      **→ I1.**
- [ ] `AUTH_POLICY` is repository-specific but lives in the crate
      (`utils/openapi-sanity/src/auth.rs`). Against any other router tree the
      checker emits one "auth policy entry … names an operation the document does
      not declare" per entry — about sixty findings for a two-operation spec.
      Decide whether the crate is single-repository, which the crate docs and the
      README should then say, or whether the policy becomes a file the repository
      owns and the checker loads; either way report the unclassified remainder as
      one summary instead of one line per entry. **Unowned** — no implementation
      item; it is a design decision, not work the invariant requires.
- [ ] `/get/test/` is written out where it must agree across three places:
      `openapi_public::TEST_ONLY_PATH_PREFIX`, the `--exclude-prefix` argument of
      the `openapi-sanity` recipe in the `justfile`, and the prefix each test
      passes. A second test-only prefix added to only some of them is either
      stripped from the artifact and still gated, or gated and published. Add a
      test that reads the recipe and asserts it matches the Rust constant. Low
      effort. **→ I3**, which supersedes the test: one policy definition read by
      all three sites removes the duplication rather than policing it.

#### Considered and rejected

Recorded so the next reader does not re-litigate them. Each was a reasonable
first instinct that the invariant framing ruled out.

- **A runtime route inventory as the CLI's input.** Feed `openapi-sanity` a route
  table dumped by the server and let it be the authority on what the backend
  registers. Rejected: it costs the checker its no-compiled-artifact property (the
  whole reason it can sit in the pre-commit hook) and makes the external tool
  responsible for the backend's registration — the checker would be judging the
  code it is meant to check. The `syn` static scan stays the CLI's method; the
  product-build route table is a _separate_ derivation used by `--check-openapi`,
  not an input to the CLI.
- **Promoting parity to a launch-time check (fail boot if the spec is missing or
  drifted, `--waive-openapi` to skip).** Rejected: `openapi.json` is a review
  artifact, not a deployment dependency. Making the server refuse to boot because
  the file is absent turns a governance check into a runtime coupling and gives
  every deployment a new way to fail to start. `--check-openapi` keeps the spec
  dependency in the repo and the release gate, where the claim is actually made.
- **One spec per feature configuration** (`openapi-embed.json`,
  `openapi-default.json`). Rejected: it doubles the artifact count and the review
  surface, and a route present in one slice and absent in another is drift between
  two files rather than between code and spec. A single canonical spec with
  `x-picasu-feature` markers keeps one source of record, and the asymmetric parity
  rule expresses "this build may expose a subset" without a second file.
- **Deriving the scanned file list from the `mount()` calls in `builder.rs`.**
  Rejected: it just relocates the hardcoded list — it still asserts which files are
  the API rather than deriving it, and a `routes![]` block not yet wired into a
  mount would be invisible to the generator but visible to the runtime check. The
  walk derives the set from the source tree instead.

## Progress

- 2026-09-28: Gave the operation-detail findings an owner. I1–I3 covered only
  the route-set half of the invariant; the four detail findings had no
  implementation item, so the plan claimed coverage it did not schedule. Added
  **I4** (compare parameter and body types) and **I5** (fix the two rules that
  answer wrongly rather than staying silent), both independent of I1–I3 since
  they touch only `params.rs` and `handlers.rs`, so they can run in parallel with
  the walk chain rather than queued behind it. Marked mechanism 3 partial in the
  strategy list: among responses only the 401 contract is checked, and among
  parameters only names, sets and `required`. Noted that I4 is design work, not a
  table addition — the `value_type` overrides on enum variants mean a naive rule
  false-positives on every such field, so the override surface should be measured
  before the rule is written, and a documented supported subset may beat a rule
  that is right about most routes and noisy on the rest. Every finding now points
  at its owning item, or is marked unowned: `AUTH_POLICY` staying in the crate is
  a design decision the invariant does not require, and remains open. The status
  family rule (405 against the 20 `GuardReadOnlyMode` routes, one of which
  declares it) is still undecided and is not covered by I4 or I5.

- 2026-09-28: Consolidated Step 5 and documented the flow. Added an
  **Implementation plan** reducing the step to three ordered pieces — I1 walk the
  backend source and retire `SCANNED_MODULES`, I2 `--check-openapi` route-set
  parity, I3 shared exclusion policy — each with the concrete files, the new
  parsing logic (generalizing `group_and_module` to arbitrary module depth), the
  acceptance condition, and the surfaces that change with it. Added a
  **Considered and rejected** record so the runtime-inventory-as-CLI-input,
  launch-time-enforcement, per-feature-spec, and derive-the-list-from-`mount()`
  directions are not re-proposed. Rewrote `docs/openapi-generator.md` around the
  invariant: it led with the checker as the product, described the symmetric
  runtime parity test as load-bearing, and instructed adding new route modules to
  `SCANNED_MODULES` — all three now wrong. The doc now leads with the invariant
  and its two halves, documents the three-check gate including `--check-openapi`
  and the asymmetric feature rule, marks feature-dependent operations with
  `x-picasu-feature` via utoipa `extensions(...)`, and carries the same rejected
  directions. The crate README and the `justfile` recipe still name the
  `--router-root`/`--module` flags; they change with I1 and are listed there
  rather than edited ahead of the code.

- 2026-09-28: Reframed the plan around the invariant rather than around the
  checker. The goal is one property — **the generated OpenAPI content must match
  what the backend implements at runtime** — and the gates are the means of
  enforcing it, not the product. It splits into two halves with different ground
  truth: the route set (Rocket's table is the authority, so it is provable at
  runtime) and operation detail (annotations only, so it is statically derivable
  against the handler source). This supersedes three earlier framings: the
  strategy no longer treats the five mechanisms as the deliverable, mechanism 2
  no longer prefers a route inventory or runtime route metadata over source
  discovery (that conflicts with the gate's shape settled below — the CLI keeps
  `syn` static analysis; runtime is the authority for the route set, not a
  replacement for the scan), and Step 5 is grouped by which half of the invariant
  each finding threatens rather than by severity. The earlier claim that the gate
  "held up" is narrowed: the methods held up, but the static side's coverage is
  asserted, not derived. A finding was added, then sharpened: route-set parity is
  proven in the product build, not in a test. Behind a `--check-openapi` flag,
  the real `build_rocket().routes()` is compared against the committed
  `openapi.json` and the check fails on drift; the spec dependency is confined to
  the repo, the commit hook, CI and the release gate, so the server's boot stays
  independent of a review artifact. The two `openapi_contract.rs` parity tests
  are therefore wrong as the load-bearing check and retire to negative
  self-checks; the CLI's route-table comparison is redundant with the build-time
  one.

- 2026-09-28: Settled Step 5's first three items after the review of the gate's
  shape. The static side was always meant to find registrations by `syn` AST
  analysis and does; the defect is that it parses only `SCANNED_MODULES`, so
  coverage is asserted rather than derived, and `builder.rs` — which mounts
  `/assets` at startup — is not among them. A runtime route inventory exported
  from the server was considered and rejected as the fix: it would cost the
  checker its no-compiled-artifact property and make the external tool the
  authority on what the backend registers. The static scan and Rocket's runtime
  route table stay as two independent derivations whose agreement is the
  evidence, and the walk replaces the list.

- 2026-09-28: Reviewed the branch against the goal it states — routes annotated
  and documented, authentication tracked, inputs and outputs described — and
  recorded what the probe found as Step 5, each item as something to confirm
  before fixing. The gate's structure held up: source scan against the
  committed artifact, Rocket's mounted route table against the served one, and
  negative self-checks for each. The gaps are coverage rather than correctness
  — a `routes![]` outside `SCANNED_MODULES` is invisible to the source gate,
  parameter types are never compared, an unnamed body type is reported as
  drift, and the response side is largely ungated, with the largest data
  response (`/get/metadata/{asset_id}`) naming no schema. Two claims in this
  plan did not survive checking: the route that was said to be missing from the
  spec (closed above) and the note that route coverage was already complete.

- 2026-09-28: Reviewed every unchecked task against the current tree and the
  committed artifact. Closed the `renew-hash-token` task as
  premise-not-reproducible: the route is annotated, mounted, in the generated
  paths, and has been in `openapi.json` since its first commit — at writing
  time there was no committed spec (gitignored) and no `openapi.rs`
  (build.rs-generated), so the claim came from a stale artifact, and the
  coverage test it asked for now exists. Refreshed the other task bodies with
  current measurements: 205 description-less reference rows / 36 of 45
  schemas; `PrefetchReturn.resolvedShareOpt` carries `oneOf: [null, …]` in the
  artifact while the reference still renders `any`/`{}`; six operations (not
  four) carry multi-line summaries that split the generated headings;
  `AlbumIndexState` is the last bare enum (`OnConflict`, `AssignOutcome`
  documented).

- 2026-09-28: Implemented Step 4 as `check_params`, the fifth rule group,
  chained into `openapi-sanity check` after the contract, tag and auth rules.
  It follows the checker-not-generation decision above: P1/P2 compare the
  sets a Rocket URI binds against the spec's placeholders and the
  operation's declared path/query parameters, with `required` checked
  against `Option<T>` on the bound argument; P3 compares the declared body
  schema name with the type `data = "<x>"` binds after unwrapping
  `Json<T>`/`Form<T>`, and requires a `multipart/form-data` content type for
  form inputs; P4 asserts the document's `operationId` equals the handler
  name (stability across releases stays mechanism 4's job); P5 fails `$ref`s
  to undefined component schemas and schemas nothing references. The 25
  findings the rules predicted on the repository — 10 undocumented path
  parameters, 11 undocumented query parameters, three `Value`/`Form` bodies,
  the `FileEntry` orphan — were fixed in the same change: parameters
  annotated on the utoipa paths, `UploadForm`, `RegenerateThumbnailForm`
  and `Expression` given `ToSchema` (`no_recursion` on `Expression`'s three
  nesting variants, without which schema composition recurses until the
  stack overflows), and `FileEntry` stripped from the public artifact
  alongside the probe schemas that are what registers it. `tests/params.rs`
  now requires the repository to be clean instead of pinning the old
  counts, the README and `docs/openapi-generator.md` document the rule
  group and its remaining blind spot (schema _content_ is still unchecked),
  and `just openapi-check` runs the gate.

- 2026-09-27: Documented the tool and pinned its known issues (Step 6).
  `utils/openapi-sanity/README.md` covers purpose, the failure class it exists
  for, CLI and library usage, the three rule groups, how it is tested, and
  limitations — including that the vocabulary (`SCANNED_MODULES`, `AUTH_POLICY`,
  `KNOWN_GUARDS`, `KNOWN_TAGS`/`DATA_API_PREFIXES`) is picasu-specific and
  compile-time rather than configuration, that source analysis cannot prove what
  Rocket mounts, and that `cfg`/features, schemas, parameter agreement,
  `operationId` stability and reachability are not checked. `tests/regressions.rs`
  adds four incident-shaped tests where rule-level coverage existed but the
  incident did not: the group-root one-line `routes![]` that lost both renewal
  routes end to end, the sibling annotation credited versus unannotated
  diagnostic, the `get_rows` discarded timestamp guard with a correct annotation
  and document, and a crate-side assertion that every `SCANNED_MODULES` entry
  names an existing file. Each asserts the exact finding and silence in the
  conforming shape; neutering the discard rule fails the incident test. The
  README is now in the `docs-check` prettier glob, and precommit runs
  `docs-check` for staged `utils/**.md`.

- 2026-09-27: Moved the tag taxonomy into the shared policy library (Step 4 of
  the earlier wording — the tag migration promised by Step 3's last bullet, not
  the parameter work the section is otherwise named for). `utils/openapi-sanity`
  gained a `tags` module: `KNOWN_TAGS` is the single declaration of the vocabulary
  and `check_tags` holds a document to it in four rules — an operation with no
  tags, a tag outside the vocabulary, `pages` on a data-API path, and `pages`
  missing from an SPA page path. `SpecOperation` carries `tags` so the rules read
  the committed document the same way the auth rules do, and `openapi-sanity
check` now merges the tag findings into the one report, so `just openapi-check`
  phase 1 covers the taxonomy without compiling the backend. The rules are
  independent, so an untagged page path is reported twice; that is deliberate,
  since it is missing a tag _and_ missing the reserved one. The document-side
  message shape is the auth rules' — `METHOD PATH: …` inside a `file:` label —
  and the verb is upper-cased like every other diagnostic in the crate, where the
  backend's own copy printed the raw JSON key.

  The migration is a move, not a rewrite: the vocabulary, the four rules and the
  `pages` placement logic are the ones `every_operation_carries_a_known_tag`
  enforced, and equivalence was measured before anything was deleted. Three
  mutations of the real router — `timeline` → an unknown `metadata`, the `tag`
  line removed, and `timeline` → `pages` on `GET /get/get-data` — each regenerated
  the artifact and were then run through the old backend test and the new CLI
  rules. All three were caught by both, on the same tree, with the same rule
  (`get /get/get-data: unknown tag \`metadata\``before,`GET /get/get-data:
  unknown tag \`metadata\``after). The same three were re-run after the migration:`cargo test --lib`now reads`check_tags` and fails with the CLI's diagnostic, so
  tag drift is not detectable only through the CLI.

  The backend keeps the document half of it, the Step 3 pattern:
  `the_public_operations_follow_the_shared_tag_taxonomy` runs `check_tags` over
  the generated public spec, with `self_check_detects_tag_drift_in_the_public_spec`
  proving the shared check still notices from where it is called. The six old
  self-checks over hand-written JSON are gone — the equivalent coverage now lives
  in the crate, where the rules are, over an `untagged/` fixture tree that carries
  one instance of each failure mode. The mounted-route parity tests and the
  `Unauthorized`-component tests are untouched: parity is runtime-only, and the
  component tests are document shape the CLI does not duplicate.

  The fixture documents gained the `tags` their annotations already declared —
  the fixtures' sources were tagged all along and only their documents were not,
  so the CLI's report over them would otherwise have changed for an unrelated
  reason. `KNOWN_TAGS` is a hand-written claim in both directions now: besides the
  gate, `the_repository_carries_the_tags_the_taxonomy_names` requires the subjects
  the committed document uses and the subjects the vocabulary names to be the same
  set, so a vocabulary entry with no operation cannot rot in place. One convention
  is not a rule: "exactly one tag per operation" is held by
  `the_repository_gives_every_operation_exactly_one_subject` rather than by a
  check, because the four rules are about an absent, an unknown and a misplaced
  tag and inventing a fifth was not part of a move.

  Gated by 13 tests over the `clean/` and `untagged/` trees (each rule as an exact
  diagnostic plus the whole report, the excluded prefix, the vocabulary itself, and
  two over the committed document), 4 new mutation tests that break one tag in a
  conforming copy and restore it, 1 new CLI test, and 1 new backend test. Every
  rule, the excluded-prefix skip, the page/data classification, the sort and the
  CLI wiring were each neutered in turn and failed a named test; both source files
  were restored byte-identically afterwards. The repository is clean: 61
  operations, 0 findings, and `backend/openapi.json` and every `#[utoipa::path]`
  are unchanged. Step 5 (parameters and documentation) is untouched.

- 2026-09-27: Aligned the authentication policy with the implementation (Step 3).
  `utils/openapi-sanity` gained a `guards` module and an `auth` module, and the CLI
  now runs both checks and prints one merged report. A handler's parameters are
  read for request guards: a bare or referenced `GuardX` is a direct guard Rocket
  runs before the body, a `GuardResult<X>`/`Option<X>` is deferred and only as
  strong as the body makes it — propagated with `?`, returned, matched, or read
  through `is_ok`/`map_err`/`expect`. A wildcard binding (`let _ = auth;`) and a
  binding the body never mentions are both reported against the parameter's line,
  which is the shape `get-rows` and `get-scroll-bar` had before
  `bug-get-rows-auth-guard-discarded.md` fixed them. `KNOWN_GUARDS` is the single
  place a guard type enters the analyzer, and a test reads the backend's
  `FromRequest` implementations to hold it to that claim in both directions.
  Eight guards, not the seven the plan listed: `POST /post/renew-hash-token` is
  guarded by `TimestampGuardModified` — a direct guard accepting an _expired_
  bearer token, since issuing a fresh one is the point — so an operation already
  in the shared-401 policy had no class to name. `GuardClass::ReadOnlyMode` is the
  one guard answering 405, which is why the policy names classes instead of
  counting guards.

  `AUTH_POLICY` is a table with **one entry per documented operation** (61, one per
  line in `utils/openapi-sanity/src/auth.rs`), keyed by `operationId`. Both
  choices are deliberate and documented in the module: listing only the public
  operations would leave the protected set implicit, so deleting a guard from a
  protected handler would leave the policy untouched and the route open — the drift
  the policy exists to catch; and a path key would make every route rename read as
  a new operation (old entry stale, new one unlisted) even though authentication
  did not change. A handler rename does change the id and fails loudly as a stale
  entry instead. Six findings, each a stable `file:line: message`: a protected
  operation with no observed guard, a handler declaring guards the policy does not
  list, a guarded operation documenting no 401, a public operation documenting one,
  an operation in no policy entry, and a policy entry no operation answers to —
  plus the discarded binding from the source scan. A `security` requirement counts
  as documenting the rejection alongside a `401` response, so the rule does not
  have to be revisited when the document gains schemes.

  The repository is clean on the first run: 61 operations, 0 findings. The
  migration of `GUARDED_OPERATIONS` in `backend/src/tests/openapi_contract.rs` is
  partial and deliberate. The two list-based 401 tests are gone, replaced by
  `the_auth_policy_and_the_documented_unauthorized_responses_agree`, which reads
  `AUTH_POLICY` instead of a second list and keeps `cargo test --lib` failing on
  the same drift — verified by removing `(status = 401, response = Unauthorized)`
  from `get_data` and watching it report `GET /get/get-data: the auth policy says
it can answer 401 but it documents none`. The tag policy is _not_ migrated: the
  new gate does not cover tags, and the plan forbids inferring security from them.
  The `Unauthorized`-component checks and the mounted-route parity tests stay in the
  backend regardless — the former is document-shaped, the latter is runtime-only.

  Gated by 24 guard-detection tests, 15 auth tests over a new `unauthored/`
  fixture tree that carries one instance of every auth failure mode (asserted rule
  by rule and as a whole report), 4 mutation tests that break one thing in a
  conforming copy and restore it — a guard removed, a `GuardResult` dropped, a
  public operation unlisted — and the existing 14 CLI tests, one of which now
  covers the merged report with a shared finding printed once. `the_repository_
matches_its_own_policy` runs the policy over the real router, and
  `the_policy_lists_every_documented_operation_and_nothing_else` pins the count.
  `backend/openapi.json` and every `#[utoipa::path]` are unchanged; no inconsistency
  was found in the current source. Step 4 (parameters and documentation) is
  untouched.

- 2026-09-27: Made `just openapi-check` the unified contract gate (Step 2).
  `utils/openapi-sanity` gained a binary, `openapi-sanity check`, and a
  `contract` module; `just openapi-check` is now two phases, the semantic one
  first — annotations vs. `backend/openapi.json` on source, no compiled backend
  needed — then the existing generated-artifact diff, and a failing dependency
  stops the recipe, so either phase fails nonzero with its own diagnostics
  visible. `just check` and CI pick it up unchanged. Seven checks, each
  reported as a stable `file:line: message` and sorted by file, line and
  message: a registered handler with no `#[utoipa::path]`, a route URI and an
  annotation path that disagree after `to_spec_path`, a route attribute and
  annotation verb that disagree, a source operation the document omits, a
  document operation no scanned source declares, a handler identity registered
  twice, and a duplicate `operationId`. Two things the library did not have:
  `Handler::spec_method` (the annotation's verb — utoipa takes it as a bare
  identifier among the annotation's top-level tokens, and it decides which
  operation the handler is registered under) and `HandlerRef::line` (so a
  duplicate registration points at the `routes![]` entry, not at the handler).
  `SCANNED_MODULES` moved from `build.rs` into the crate rather than being
  forked, which surfaced a stale entry: `("fairing", "fairing/mod.rs")` has
  listed a file that has not existed since c6e599a9 flattened
  `router/fairing/*` into `router/auth.rs`, and the build script skipped it
  silently. Removed, with `every_scanned_router_module_exists` added to
  `backend/src/tests/route_scan.rs` so a dead entry cannot come back;
  `openapi.rs` and `openapi.json` regenerate byte-identically. A handler whose
  annotation disagrees with its own route is reported once, locally, and is not
  also reported as document drift — the document inherits the disagreement from
  the annotation, so a second finding would restate the first; the drift fixture
  pins one finding per rule. The exclusion that keeps the test-only probes out
  of the comparison is a `--exclude-prefix` argument rather than a constant in
  the analyzer, because it describes the artifact and not the analysis;
  `just openapi-check` passes `/get/test/`, and running without it is a test.
  The repository is clean: 61 operations, 0 findings. Gated by 20 checks over
  the `clean/` and `drift/` fixture trees in `utils/openapi-sanity/tests/`
  (each rule as an exact diagnostic, plus the whole report), 14 CLI tests, and
  one that runs the gate over the real `backend/src/router` and
  `backend/openapi.json` so it cannot be neutered and stay green. Step 3
  (auth policy) is untouched.

- 2026-09-27: Extracted the source analyzer into `utils/openapi-sanity`
  (Step 1). The crate owns the `routes![]` scan, the per-function
  `#[utoipa::path]` attribution, Rocket route/URI discovery and the
  Rocket-to-OpenAPI path translation; `build.rs` keeps only what needs the
  filesystem — which router files to scan, and writing `openapi.rs` — and
  `openapi_contract.rs` imports the translation instead of defining a second
  copy. `backend/build/route_scan.rs` and its `#[path]`-included test module
  are gone: the parser whose bug made the renewal routes undocumented is now
  covered by 33 black-box tests against the crate's public API. Behaviour
  differs from the text scanner it replaces in four ways, all of them
  deliberate — attribution is per function (a file-level check can credit a
  sibling's annotation), an entry that is not `ident`/`path::ident` becomes a
  finding instead of a silent skip, a syntax error becomes a file/line finding
  instead of a partial scan, and `source` must be a complete file rather than
  a fragment. `backend/src/openapi.rs` and `backend/openapi.json` regenerate
  byte-identically: the per-function check credits exactly the handlers the
  old substring check did for the current router files. `scan_source` parses
  once for both views and exists for the Step 2 CLI, so it is covered by
  tests rather than by `build.rs`.

- 2026-09-25: Tagged every public operation and gated the taxonomy (Medium
  "Tag every operation"). Each `#[utoipa::path]` annotation got one
  `tag = "..."` line (`pages` kept on the 22 SPA routes, `albums` kept on
  `assign_album`); 38 operations were untagged before. Final distribution:
  `pages` 22, `albums` 9, `assets` 7, `config` 6, `timeline` 6, `index` 5,
  `auth` 3, `serving` 2, `upload` 1 (61 total). The plan's suggestion list was
  off: `metadata`/`shares` were folded into `assets`/`albums`, and `timeline`
  (grid data) had to be added; `pages` was already in use. The gate is
  `every_operation_carries_a_known_tag` with the vocabulary in a `KNOWN_TAGS`
  constant and the `pages` expectation derived from data-API path shapes (the
  spec alone cannot say which file annotated an operation — assumption
  documented on `is_data_api_path`). Three mutations (drop the missing-tag
  report, drop the unknown-tag report, drop the data-API/`pages` report) each
  failed their named self-check before reverting. Taxonomy table and
  enforcement noted in `docs/openapi-generator.md`.

- 2026-09-25: Documented the 401 contract. The sweep exposed a real security
  gap rather than only a documentation one: `GET /get/get-rows` and
  `GET /get/get-scroll-bar` discarded `GuardResult<GuardTimestamp>` with
  `let _ = auth;`, so both answered 200 to an unauthenticated caller, and
  `/get/get-scroll-bar` panics on an unknown snapshot id without credentials.
  Recorded as `bug-get-rows-auth-guard-discarded.md` and fixed separately, since
  it is a behavior change rather than documentation: both handlers now
  propagate the guard, both operations declare 401, and two scenarios assert
  401 for missing, malformed and expired tokens. The contract
  tests for 401 are bidirectional: an operation declaring a 401 must be listed in
  `GUARDED_OPERATIONS`, and a listed operation that stops existing fails with a
  stale-entry message instead of a missing-declaration one.
- 2026-09-25: Added negative self-checks so the gates cannot be neutered
  silently. The `routes![]` scanner moved out of `build.rs` into
  `backend/build/route_scan.rs`, shared with `src/tests/route_scan.rs`, because
  a build script cannot be unit-tested in place — its parsing bug is exactly
  what made the renewal routes undocumented. The parity comparison moved into
  pure functions (`undocumented_routes`, `stale_operations`,
  `duplicate_operation_ids`) that `src/tests/openapi_contract.rs` exercises with
  deliberately drifted inputs, and a new test asserts `public_json()` equals the
  committed `backend/openapi.json`, so artifact drift fails `cargo test` and not
  only `just openapi-check`. 13 mutations (neuter each comparison, drop the
  exclusion filter, widen the exclusion, ignore the HTTP method, no-op the path
  normalizer, revert the scanner to line-based parsing, accept non-identifier
  entries, stop stripping comments, end the block at the first bracket, scan
  only the first block, stale artifact) were each verified to fail a named test.
- 2026-09-25: Established hardening mechanisms 1 and 2.
  `backend/openapi.json` is now a committed, pretty-printed, sorted-key
  artifact; `just openapi-check` regenerates and diffs it and is part of
  `just check` (so CI and the `main` pre-commit hook run it).
  `backend/src/tests/openapi_contract.rs` compares Rocket's mounted route table
  with the spec: undocumented mounted routes, documented-but-unmounted
  operations, duplicate `operationId`s, and dead contract exclusions all fail.
  Rocket's `<segment>`/`<segment..>` URI syntax is normalized to OpenAPI
  `{segment}` templates; the test-only probes and the `/assets` file server are
  explicit, reviewed exclusions. Both directions were verified by injecting
  drift: renaming an annotated path fails `every_spec_operation_is_mounted`,
  and un-scanning `router/auth.rs` fails `every_mounted_route_is_documented`.
  Spec operation count went 59 → 61. Remaining: structural linting (3),
  breaking-change detection (4), spec-driven contract smoke tests (5), plus the
  content tasks above. The markdown reference is still not drift-checked
  because `widdershins` is fetched over the network.
- 2026-09-26: Consolidated the proposed `openapi-sanity` plan into this task.
  `openapi-sanity` was an implementation name for the shared source/spec/auth
  work, not an independent feature. The unified gate is `just openapi-check`:
  semantic source/spec checks first, then generated-artifact diffing. Runtime
  parity, endpoint scenarios, Markdown generation and request reachability
  remain separate owners.
- 2026-09-23: Rework-adjacent subset executed by the path-primary cleanup
  sweep (see `.plan/path-primary-cleanup.md`): test-only probes stripped from
  the public spec; `PUT /put/assign_album` given tag `albums`, a single-line
  summary and explicit description (fixes its mangled reference anchors);
  `OnConflict` and `AssignAlbumData.albumId` documented. Reference regenerated.
- 2026-09-23: Dependency for task 1 (`renew-hash-token`): decide
  `path-primary-cleanup` category 7/B3 first — if the route is renamed
  hash→asset token, register the final path instead of documenting the old one
  and then changing it.
- 2026-09-24: Superseding the B3 dependency note: `path-primary-cleanup`
  category 7/B3 was withdrawn (content hash intentionally stays in compressed
  URLs and token claims; route remains `/post/renew-hash-token`). Task 1's
  rename dependency is resolved — it may proceed at any time via the
  `build.rs` `router/auth.rs` route-scan fix.
