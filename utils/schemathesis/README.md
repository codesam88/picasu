# Schemathesis fuzz harness

## Tier 1 — check

Runs a bounded single-operation sweep against a throwaway seeded backend and
compares findings to `baseline.yaml`.

Run:

    just fuzz-check

What it does:

- builds the backend and snapfab fixtures
- seeds a disposable instance (fresh dirs, index, restart, shares)
- runs schemathesis with the fixed 5-check oracle
- compares the JSON report to the accepted-findings baseline

Updating the baseline when a bug is fixed: remove the corresponding entry
from `baseline.yaml` and re-run; a fixed bug must make that finding disappear.

State lives in a temp dir removed on exit. Set `FUZZ_REPORT_OUT=<path>` to
copy the JSON report out before teardown (used by CI for SARIF conversion).

## Tier 2 — campaign

Stateful run against a _running_ seeded backend. It does not seed or tear
down an instance itself.

Run against a disposable instance:

    export FUZZ_STATE_DIR=$(mktemp -d)
    PICASU_PORT=5673 utils/schemathesis/seed.sh
    source "$FUZZ_STATE_DIR/seed.env"
    just fuzz --max-time 120

Or against a developer instance (default `http://localhost:5673`):

    just fuzz

Fixture/env requirements: `PICASU_JWT`, `FUZZ_ALBUM_A`, `FUZZ_SHARE_A`,
`FUZZ_ALBUM_B`, `FUZZ_SHARE_B`, `FUZZ_PASSWORD`, `PICASU_BASE_URL` (all
provided by `seed.env`, which also points the recipe at the seeded port).
The campaign targets a running instance, so destructive operations are
disabled in `campaign.toml` (`update_admin_config_handler`,
`reset_auth_key_handler`, `disable_read_only_handler`, index rebuild,
logout, user password/admin mutations) and the hooks' poison guards null
config fields, protect the admin, and redirect share/delete operations to
nonexistent fixtures.

What A1-A5 assert:

- A1 `cross_scope_refused` — a share-A snapshot token cannot read an album-B
  asset.
- A2 `demoted_token_rejected` — a demoted user's still-live token loses admin
  access immediately.
- A3 `wrong_typ_rejected` — an identity token does not satisfy a
  snapshot-token guard.
- A4 `readonly_405` — every mutating operation answers 405 under read-only
  mode (env-gated via `FUZZ_READONLY=1`).
- A5 `cross_share_renewal_refused` — share B cannot renew a snapshot minted
  for share A.

Budget: default `--max-time 900s`, overridable via pass-through args.
Stateful phase uses up to 8 steps per sequence.

The campaign runs in three env-gated modes controlled by
`FUZZ_SHARE_HEADERS`, `FUZZ_MARKERS`, `FUZZ_READONLY` (default-on for the
first two in `just schemathesis-campaign`).

## SARIF / Security tab

`utils/schemathesis/json2sarif.py <report.json> <baseline.yaml> <out.sarif>`
converts a run report to SARIF 2.1.0. Baseline-accepted findings are
excluded; results are advisory (`level: warning`). Operation anchoring is
line-based against `backend/openapi.json`. The nightly workflow uploads
the result under the `schemathesis` category.
