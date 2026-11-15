---
status: open
type: feature
priority: medium
area: backend
---

## Notes

Review and revise the devops layer: justfile recipe structure, gate
taxonomy, generated-artifact policy and CI responsibilities. Motivation:
the quint restructure (utils/quint/justfile, 2026-11-14/15) established
a working pattern — a per-tool module with a harmonized verb set — that
the rest of the repo does not follow yet, and the generated-artifact and
CI wiring grew gate-by-gate without a policy.

Current state (2026-11-15, verified):

- Recipe structure: root justfile carries ~70 flat recipes. Only `bench`
  and `quint` are submodules (`mod`). The utils crates (`snapfab`,
  `paste`/paste-shim, `openapi-sanity`) are wired through flat
  `utils-format`/`utils-check`/`utils-test` enumerations — every new
  utils crate edits three lists (and the quint restructure had to touch
  all three).
- Verb taxonomy: backend/frontend/docs use `format`/`check`/`test`;
  utils crates have no lint/format split (clippy+fmt in one
  `utils-check`); the quint module already harmonizes
  `lint`/`format`/`check`/`test`/`fulltest`/`selftest`/`docs`.
- Generated artifacts, each committed with its own staleness check and
  manual regen command: `backend/openapi.json` (openapi-json-match),
  `docs/quality-gates.md` (gates-check/gates-docs),
  `docs/eventual-consistency.md` (doc-check/docs),
  `docs/auth-matrix.md` + `backend/auth-matrix.json`
  (auth-matrix-check), `utils/quint/pipeline/traceability.json`
  (specs-check staleness message). The traceability staleness gate was
  the most frequently hit friction of the 2026-11-13..15 arc (four+ interruptions).
- precommit: on main = full `check` + `test`; on branches = targeted
  checks per changed path (backend/utils/quint/docs/frontend branches),
  with no generated-artifact auto-regen anywhere.
- CI: PR lanes run backend-check/utils-check/openapi-check/env-var-check/
  gates-check, fuzz-check, frontend/docs checks, dylint-check (advisory).
  No quint model runs, no fulltest deep lane, no fuzz-matrix in CI —
  model proofs and fuzzing are local-only today.
- Checker-layer RoI items from the 2026-11-15 review fold in here:
  shared `.qnt` declaration parser (operations-check mirrors
  specs-check), schema id-enum de-churn (hand enums in
  catalog.schema.json required an edit per new G/REQ id three times),
  coverage matrix as a generated report.

## Goal

One recipe taxonomy, applied uniformly:

- Every `utils/*` tool gets a submodule justfile like bench/quint with
  the harmonized verbs: `format`, `lint`, `check`, `test`, `gen`
  (where the tool generates artifacts), optional `selftest`/`fulltest`.
- Root aggregates (`check`, `test`, `precommit`, CI) compose the modules
  instead of enumerating flat recipes; adding a utils tool stops
  requiring edits to three root lists.
- Generated-artifact policy, stated once: which artifacts are committed
  (reviewable) vs generated-at-check-time; committed ones are
  auto-regenerated at precommit (or in the PR lane) instead of failing
  staleness gates; regen failures surface as review, not as blocked
  commits. Applies to openapi.json, quality-gates.md,
  eventual-consistency.md, auth-matrix artifacts, traceability.json.
- Gate taxonomy: `check` = fast static/structural (any code change);
  `test` = behavioral incl. model runs; PR gates = check + targeted
  tests; nightly/main = fulltest, fuzz-matrix, audits, (optionally) a
  quint proof lane.
- Fuzzing: decide whether `utils/schemathesis` becomes a module like
  bench/quint (verbs: `check`, `test`/`matrix`, `serve`) or stays
  script-driven; fuzz jobs get an explicit CI slot (PR-lane smoke vs
  nightly campaign).
- Quint proof jobs: fulltest (elevated samples) as the nightly proof
  lane; decide explicitly whether model checking (`quint verify`) has a
  place in CI — the modeling workflow deliberately keeps verify out of
  the standard loop (minutes-to-hours), so a proof lane would be an
  opt-in scheduled job, not a gate.

## Steps

1. Survey: full recipe/caller inventory (quality-gates.mjs output),
   generated-artifact list with owners, CI lane map (PR vs main vs
   nightly), precommit branch coverage.
2. Design: module pattern spec for utils tools, verb taxonomy, generated-
   artifact policy (commit-vs-generate per artifact), gate/CI lane map.
   Sign-off on: gen auto-regen at precommit vs PR; proof/fuzz lanes.
3. Implement module justfiles for the utils tools; root aggregates
   compose modules; remove the flat enumerations.
4. Generated-artifact policy pass: precommit/PR auto-regen where decided;
   staleness gates replaced or kept deliberately (with rationale).
5. Checker-layer items: shared .qnt declaration parser, schema enum
   de-churn, coverage matrix as generated report.
6. CI: move PR lanes to `check` + targeted tests; add nightly lane
   (fulltest, fuzz-matrix, audits); keep dylint advisory; update
   quality-gates doc.
7. Validate: full `check`/`test` green; selftests green; the
   add-a-utils-crate scenario exercised (no root-list edits needed).

## Open questions

- `gen` vs `generate` as the verb name (quint module currently has
  `docs`/`doc-check` — rename to `gen`/`gen-check` for uniformity?).
- Is `traceability.json` still worth committing once auto-regen exists,
  or does it become check-time-only state?
- Does the nightly proof lane run `quint verify` (model checking) on a
  schedule, or only fulltest sampling? (Verify is exhaustive and slow;
  the current models are sampled-only by policy.)
- Does fuzzing move into a submodule justfile or remain script-driven
  under utils/schemathesis?
- dylint-check: advisory CI-only today — keep, or fold into a lint lane
  behind a toolchain check?

Reference: utils/quint/justfile (the module pattern), utils/bench/justfile,
.plan/quint-annotation-crate.md (gate layering), AGENTS.md (workflow).

Done pattern (2026-11-15): utils/quint now carries two subfolders —
`quint-annotate/` (proc-macro crate, package name `quint`) and
`coverage/` (TS validation tool, dispatcher `coverage/bin/check.ts`) —
alongside `pipeline/` and `tests/`, validating the utils-tool-as-submodule
layout.
