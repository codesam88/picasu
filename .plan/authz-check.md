---
status: backlog
type: feature
priority: high
area: backend
---

## Notes

Structural assurance for the authentication subsystem: convert the invariants
behind `docs/auth.md` (Security assessment, findings F1–F10) from conventions
into machine-checkable statements, so a violation cannot merge. The rationale
and verified exploit transcripts live in that document's assessment; this plan
is the work item. Five components, in dependency order:

### 1. Policy artifact (semi-formal authz model)

One machine-readable, reviewed-like-code file stating the whole authorization
design:

- principals/capabilities: admin, share + its four flags, anonymous, page
  routes — in the vocabulary of routes, not prose;
- token types: claims, TTL, mint sites, and an explicit mapping of _which
  guard may decode which type_ (the F1 defect written down where a checker can
  compare it against code);
- per-route classification: required guard set, claim-bindings implied by the
  route's inputs (which body/query fields must equal which claims fields),
  read-only applicability, share flags consulted;
- transitions (login, prefetch, renewal, logout) with pre/post-conditions —
  e.g. renewal requires presenter share == token share plus DB re-validation
  of the embedded share before re-issue (F4).

Two required properties: **authority, not documentation** — code checked
against it in both directions (route without entry fails; guards that don't
match the handler fail), the same contract `openapi-json-match` imposes on
`openapi.json`; and **bounded scope** — guards, claims, bindings, flags,
transitions only, everything else stays in code review (an over-scoped model
rots).

### 2. `authz-check` static gate

Extends the `utils/openapi-sanity` pattern — that tool already parses handler
source and fails CI on annotation/guard/source disagreement (A1–A7, P1–P4);
this adds rule families over the policy artifact:

- route–policy parity: every mounted route classified, handler guards ==
  declared set — generalizes `--check-openapi` from "route in document" to
  "route with a declared authz decision"; a new route cannot merge
  unclassified (the F9 failure mode);
- guard completeness: every mutating `#[post]`/`#[put]`/`#[delete]` carries
  `GuardReadOnlyMode` and the policy's guards (F9);
- claims consumption: a `GuardShare` handler whose inputs name an external
  resource must read the guard's claims — `let _ = auth?` before a
  body-targeted write is a finding (F3);
- decode agreement: the type parameter at every JWT decode site equals the
  type policy assigns to that guard (F1);
- secret handling: constant-time secret comparison (F5), no
  `httpOnly: false` (F7), no query-string bearer (F10), no
  `Cache-Control: public` on `/object` (F10).

One `authz-check` phase in the `just check`/precommit family, structured like
`openapi-check`'s phases: a red result blocks the change, not the release.

### 3. Enforcement idioms (checks as types)

Assurance ladder: _unrepresentable > statically checked > tested > reviewed_;
each invariant moved down it is a permanent regression reduction.

- write-handler targets constructible only from `GuardShare` claims (or
  claims + a shared comparison helper) — F3 becomes unwriteable rather than
  forgettable;
- one decode entry point per token type, each enforcing a type claim, guards
  naming their token type in the signature — F1 becomes a compile-time
  mismatch;
- declared funnels: `Claims*` minting, `validate_share_access`,
  `resolve_show_download_and_metadata` are the only sanctioned sites for their
  operations, scanner flags new ones. First enumerate sanctioned flag readers —
  `prefetch` reads `show_metadata` directly for its filter, the upload path
  reads `show_upload` directly — and treat additions as policy changes;
- deny-by-default composition: read-only/flag/expiry checks resolve through
  helpers whose fallback branch denies.

### 4. Negative tests generated from the model

Extend the existing scenario/contract-test infrastructure (the
`backend/tests/openapi_contract.rs` pattern), not a parallel framework:

- token × guard cross-product: every token type to every guard's decode path,
  only the designated type accepted — the standing F1 regression pin (the
  confusion was invisible because no test ever fed an asset token where a
  snapshot token belonged);
- principal × scope matrix: (admin, share A, share B, anonymous) × (own,
  another's) → expected refusal **and** expected absence of effect — target
  album unchanged after a refused write (F3), read-only still on after a
  refused import (F9);
- flag-preservation properties: `showMetadata=false` ⇒ no hidden field in any
  successful response; `showDownload=false` ⇒ no `allowOriginal: true` mint
  and no original bytes served (F1).

Writing rules: assert on data/side effects, not status codes (F1 and F3
returned 200 while violating policy — a status-code suite passes against the
vulnerable code); pin every reproduced finding as a named test whose exploit
transcript in `docs/auth.md` must keep failing.

### 5. Traceability and evidence ladder

Each invariant carries three artifacts changing in one review — policy entry,
enforcing idiom/rule, negative test (route without entry cannot merge; entry
without test cannot merge). Record per invariant where it is enforced (by
construction / static check / test / review) and require new work to move one
invariant down the ladder.

**Sequencing:** component 1 first — the parity gate and cross-product tests
are cheap once the artifact exists, and component 3's idioms then have a
formal target; idioms-first produces wrappers whose correctness is again only
reviewable.
