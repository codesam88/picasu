# Schemathesis fuzz harness

## Tier 1 — check

Runs a bounded single-operation sweep against a throwaway seeded backend and compares findings to `baseline.yaml`.

Run:

    just fuzz-check

What it does:

- builds the backend and snapfab fixtures
- seeds a disposable instance (fresh dirs, index, shares)
- runs schemathesis with the fixed 5-check oracle (the hooks inject the share
  headers on the `GuardPresenter`-guarded operations, derived from
  `backend/auth-matrix.json` — see "Auth knowledge derivation")
- compares the JSON report to the accepted-findings baseline

Updating the baseline when a bug is fixed: remove the corresponding entry from `baseline.yaml` and re-run; a fixed bug must make that finding disappear.

State lives in a temp dir removed on exit. Set `FUZZ_REPORT_OUT=<path>` to copy the JSON report out before teardown (used by CI for SARIF conversion).

Tier 1 gates CI (`ci.yml`, backend job): a finding outside `baseline.yaml` fails the build. It is deliberately absent from `just check`/precommit, where the seed-and-sweep runtime does not belong.

## Fuzz matrix — deterministic, gating

    just fuzz-matrix

A campaign driven by the postulated auth-model invariants rather than by
generated input: one request per operation the matrix assigns a check to, with
the status that check asserts. The check list is rebuilt from
`backend/auth-matrix.json` on every run, so guards and token types reach the
checks through the contract instead of a hand list — and there is no
committed copy to go stale.

It runs unconditionally: `just test` includes it, and CI runs it next to the
tier-1 sweep.

| Check              | Request                                                      | Assertion              |
| ------------------ | ------------------------------------------------------------ | ---------------------- |
| `cross-scope`      | share-A snapshot token against an album-B asset              | exactly `404`          |
| `wrong-credential` | admin session token as a bearer on a snapshot-only operation | exactly `401`          |
| `renew-mismatch`   | share-B credentials renewing share-A's snapshot              | exactly `401`          |
| `demoted`          | a demoted user's still-live session on admin operations      | not admitted (any 4xx) |

The demoted family asserts refusal rather than `401` because two admin
operations declare a non-guard admission path in `fallback`: registration
answers `403` (knob off, no invite presented) rather than rejecting the
credential, and the bootstrap window is a runtime condition the seeded state
does not open.

`just fuzz-matrix` runs in CI next to the tier-1 sweep, and `just test`
includes it. The campaign below keeps the generative role — sequences, random
inputs — and stays advisory.

## Tier 2 — campaign

Stateful run in two commands: one serves, one fuzzes.

### `just fuzz-serve` — serve a seeded instance in the foreground

    just fuzz-serve

First run seeds a fresh instance (bootstrap, index, shares) into the stable `sandbox/fuzz/` (repo root, gitignored); later runs restart the same state immediately. Serves `http://127.0.0.1:5673` in the foreground — **Ctrl-C stops it**. A port already occupied by another backend is refused, never hijacked. `source sandbox/fuzz/seed.env` exposes `PICASU_JWT` and the fixture ids to other tools; delete `sandbox/fuzz/` to reseed.

### `just fuzz-run` — campaign against the served instance

    just fuzz-run

Runs the campaign against `http://127.0.0.1:5673` with the served instance's auth (`sandbox/fuzz/seed.env` is picked up automatically). Budget: `FUZZ_MIN` minutes (default 15) — e.g. `FUZZ_MIN=2 just fuzz-run` for a 2-minute run; an explicit `--max-time` argument wins over `FUZZ_MIN`. Exit status is the baseline comparison — known findings pass, new ones fail, and a crashed run exits with Schemathesis's own code before comparing.

### Campaign engine (`campaign.sh`) — targeting an explicit backend

The justfile keeps only the common flows (`fuzz-check`, `fuzz-serve`,
`fuzz-run`); the underlying commands are scripts you combine yourself. The
campaign engine takes the pinned Schemathesis version first (justfile
`schemathesis_version`):

    utils/schemathesis/campaign.sh <version> --max-time 120

Runs against a backend you provide (`PICASU_BASE_URL`, default `http://localhost:5673`). Authentication options:

(a) Source a `seed.env` from a seeded instance, then run:

    source <state-dir>/seed.env
    PICASU_BASE_URL=http://127.0.0.1:5673 utils/schemathesis/campaign.sh <version>

(b) Fresh empty store: boot with `--bootstrap-admin`, create the admin, then capture the JWT:

    # server started with --bootstrap-admin --trivial-passwords
    curl -s -X POST "$BASE/post/users/bootstrap" -H 'Content-Type: application/json' \
      -d '{"userId":"admin","password":"FuzzPass!234"}'
    curl -s -D headers -o body -X POST "$BASE/post/users/authenticate" \
      -H 'Content-Type: application/json' \
      -d '{"userId":"admin","password":"FuzzPass!234"}'
    export PICASU_JWT=$(sed -n 's/^set-cookie: jwt=\([^;]*\).*/\1/pI' headers | tr -d '\r')

(c) Populated store: with the server stopped (the store is locked while it is open — "The store is locked while the server holds it open. Writes then fail with ... Stop the server before administering the store."), add an admin offline, then authenticate:

    picasu-cmd --userdb <path> user add fuzz-admin --admin --password <pw>
    # start the server, then POST /post/users/authenticate and capture Set-Cookie: jwt

(d) Auto-login via environment: set `FUZZ_ADMIN_USER` (default `admin`) and `FUZZ_ADMIN_PASSWORD`; `campaign.sh` authenticates before the run and fails loudly on a 401 or missing cookie:

    FUZZ_ADMIN_PASSWORD='FuzzPass!234' PICASU_BASE_URL=http://127.0.0.1:5673 \
      utils/schemathesis/campaign.sh <version> --max-time 30

A run without credentials or fixtures still works: it goes unauthenticated with the probes off.

The exit status is the baseline comparison — known findings pass, new ones fail, and a crashed run exits with Schemathesis's own code before comparing.

Fixture/env requirements: `PICASU_JWT`, `FUZZ_ALBUM_A`, `FUZZ_SHARE_A`, `FUZZ_ALBUM_B`, `FUZZ_SHARE_B`, `FUZZ_PASSWORD`, `PICASU_BASE_URL` (all provided by `seed.env`, which also points the recipe at the seeded port). The campaign targets a running instance, so destructive operations are disabled in `campaign.toml` (`update_admin_config_handler`, `reset_auth_key_handler`, `disable_read_only_handler`, index rebuild, logout, user password/admin mutations) and the hooks' poison guards null config fields, protect the admin, and redirect share/delete operations to nonexistent fixtures.

### Auth knowledge derivation (`backend/auth-matrix.json`)

The hooks do not carry hand lists of auth knowledge. `auth_rules.py` derives it from `backend/auth-matrix.json` (the committed machine contract of the per-operation `x-picasu-auth` annotation, regenerated and diffed by `openapi-check`):

- share-header injection set — the `GuardPresenter`-guarded operations (both tiers)
- check assignment — which operation carries which of the cross-scope / wrong-credential / renewal-mismatch / demoted checks (`test_plan`, one check per operation by priority)
- wrong-credential token selection — a token outside the operation's `credentials`
- read-only exemptions — non-GET operations without `GuardMutate`
- scope pairs — the seeded share identities crossed with the foreign albums they must not reach

The derivation is pure and unit-tested; `just utils-test` runs the suite (or `python3 utils/schemathesis/test_auth_rules.py` for the file alone), so CI and the precommit hook enforce it. The tests pin the derivations against the committed matrix, so a backend guard change fails them and forces a check review instead of silent drift. A missing or malformed matrix file fails the run (override the location with `FUZZ_AUTH_MATRIX`). The destructive-operation disables in `campaign.toml`/`check.toml` stay hand-tuned, with a unit test keeping their operation ids in sync with the matrix.

What A1-A5 assert (probe targets are generated from the matrix, not hand-picked):

- A1 `cross_scope_refused` — a share-A snapshot token cannot read an album-B asset: every snapshot-typed operation whose path binds an asset id answers 404 on the mismatch (S4 binding semantics — not 403, so the route does not reveal that the id exists).
- A2 `demoted_token_rejected` — a demoted user's still-live token loses admin access immediately: every admin-guarded session operation answers 401.
- A3 `wrong_credential_rejected` — a session-typed token does not satisfy a snapshot-token guard: every snapshot-typed bearer operation answers 401.
- A4 `readonly_405` — every mutating (`GuardMutate`) operation answers 405 under read-only mode (env-gated via `FUZZ_READONLY=1`; requires read-only mode enabled on the target instance).
- A5 `cross_share_renewal_refused` — share B cannot renew a snapshot minted for share A: the album-scoped renewal operations (AUTH-10) answer 401.

Budget: default `--max-time 900s`, overridable via pass-through args. Stateful phase uses up to 8 steps per sequence.

The campaign runs in three env-gated modes controlled by `FUZZ_SHARE_HEADERS`, `FUZZ_MARKERS`, `FUZZ_READONLY`. The first two default on (set by `campaign.sh`); `FUZZ_READONLY` stays off unless exported and the target instance has read-only mode enabled.

### Fundamental commands

For combinations beyond the two flows above (all take the pinned version
first):

    utils/schemathesis/serve.sh [picasu-args]   # what fuzz-serve runs: seed on first use, foreground
    utils/schemathesis/check.sh <version>       # tier-1 sweep + baseline compare without the recipe
    utils/schemathesis/campaign.sh <version> …  # the campaign engine (env: PICASU_BASE_URL, FUZZ_STATE_DIR, FUZZ_ADMIN_*, FUZZ_MIN)
    ./scripts/schemathesis-run.sh <version> <schema> <url> [args…]  # raw uvx passthrough, no harness config

## SARIF / Security tab

`utils/schemathesis/json2sarif.py <report.json> <baseline.yaml> <out.sarif>` converts a run report to SARIF 2.1.0. Baseline-accepted findings are excluded; results are advisory (`level: warning`). Operation anchoring is line-based against `backend/openapi.json`. The nightly workflow uploads the result under the `schemathesis` category.
