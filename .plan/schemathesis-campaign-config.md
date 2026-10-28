---
status: done
type: chore
priority: medium
area: testing
---

## Problem

The Schemathesis pilot ran from scratch (`/tmp`) with a throwaway
harness. A permanent gate needs committed campaign configuration, but
only after spec reconciliation removes the noise floor (see
`spec-reconciliation-4xx`).

## Goal

Committed, reproducible fuzz campaign: harness in the tree, links
decision (injected at load vs handwritten chains), accepted-findings
baseline, bigger stateful budget, CI wiring as an advisory gate first.

## Steps

1. Commit the harness (seed, auth hooks, custom checks, config) from
   the pilot prototype; exclude config-write ops or sandbox them.
2. Decide links: `before_load_schema`-injected (spec stays clean) vs
   handwritten chains.
3. Record the accepted-findings baseline; set the stateful budget past
   the pilot's 150 s (55/64 ops skipped).
4. Wire into CI advisory-only; promote to failing only on a clean,
   stable record.

## Outcome

All four steps done. `utils/schemathesis/` holds both tiers
(`check.toml`/`campaign.toml`, hooks, seed, `check_report.py`,
`json2sarif.py`); `just fuzz-check` is the bounded tier-1 check,
`just fuzz-run` the stateful campaign. Links: built-in inference + the
OpenAPI resource pool, no spec changes (7/8 links covered in
verification; token chains stay in hooks). Baseline carries only the
filed bug classes. `.github/workflows/fuzz-advisory.yml` runs nightly
with job-level `continue-on-error` (the one-line flip to gating) and
uploads baseline-filtered SARIF to the Security tab; first upload on
GitHub is the remaining unproven step.
