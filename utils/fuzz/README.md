# Schemathesis fuzz harness

Strategy and threat model: `docs/api-fuzzing.md`. This file covers operations only.

The command surface is `just fuzz <verb>` (`utils/fuzz/justfile`); every verb
delegates to `utils/fuzz/fuzz.sh`.

## Commands

    just fuzz          # list verbs
    just fuzz check    # tier-1 A1-A5 sweep, baseline-compared (gating)
    just fuzz matrix   # deterministic auth-matrix checks (gating)
    just fuzz roles    # per-role differential passes
    just fuzz diff     # diff the per-role reports (fails on new diffs only)
    just fuzz run      # advisory stateful campaign
    just fuzz clean    # remove the campaign state and per-role reports

`just fuzz roles --help` and `just fuzz run --help` print the harness usage,
including every option and environment default.

## Budget

Every generative verb takes a budget preset:

| Level   | `MAX_EXAMPLES` | `DEADLINE` (s) | `FUZZ_MIN` |
| ------- | -------------- | -------------- | ---------- |
| `smoke` | 3              | 15             | 1          |
| `min`   | 10             | 60             | 5          |
| `full`  | 25             | 120            | 15         |

`FUZZ_LEVEL=smoke|min|full` selects a preset. `roles` defaults to `smoke`;
`run` defaults to 15 minutes (`full`). Explicit `MAX_EXAMPLES`, `DEADLINE`,
`SEED` (roles, default 42) and `FUZZ_MIN` (run) override the preset; the verbs
also accept the same values as flags (`--level`, `--max-examples`,
`--deadline`, `--seed`, `--min`). `matrix` and `check` keep their fixed
bounded budgets.

## Tier 1 — check

Runs a bounded single-operation sweep against a throwaway seeded backend and compares findings to `baseline.yaml`.

    just fuzz check

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

    just fuzz matrix

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

The generative campaign below keeps the sequences and random-input role and
stays advisory.

## Tier 2 — campaign

    just fuzz run

Stateful run against a seeded instance. On first use it seeds
`sandbox/fuzz/` (repo root, gitignored) with the full target matrix; later
runs restart that state instead of reseeding, so repeated campaigns share one
corpus. `just fuzz clean` removes it (and the per-role reports) after stopping
a recorded server.

Budget: `FUZZ_MIN` minutes (default 15) — `just fuzz run --min 2` or
`FUZZ_MIN=2 just fuzz run` shortens a run; an explicit `--max-time` argument
wins over `FUZZ_MIN`. Exit status is the baseline comparison — known findings
pass, new ones fail, and a crashed run exits with Schemathesis's own code
before comparing.

### Targeting an explicit backend

Pass `--base-url` to skip seeding and run against a backend you provide:

    just fuzz run --base-url http://127.0.0.1:5673 --min 2

Authentication options:

(a) Source a `seed.env` from a seeded instance, then run:

    source <state-dir>/seed.env
    just fuzz run --base-url "$PICASU_BASE_URL" --min 2

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

(d) Auto-login via environment: set `FUZZ_ADMIN_USER` (default `admin`) and `FUZZ_ADMIN_PASSWORD`; the harness authenticates before the run and fails loudly on a 401 or missing cookie:

    FUZZ_ADMIN_PASSWORD='FuzzPass!234' just fuzz run --base-url http://127.0.0.1:5673 --min 1

A run without credentials or fixtures still works: it goes unauthenticated with the probes off.

Fixture/env requirements: `PICASU_JWT`, `FUZZ_ALBUM_A`, `FUZZ_SHARE_A`, `FUZZ_ALBUM_B`, `FUZZ_SHARE_B`, `FUZZ_PASSWORD`, `PICASU_BASE_URL` (all provided by `seed.env`). The campaign targets a running instance, so destructive operations are disabled in `campaign.toml` (`update_admin_config_handler`, `reset_auth_key_handler`, `disable_read_only_handler`, index rebuild, logout, user password/admin mutations) and the hooks' poison guards null config fields, protect the admin, and redirect share/delete operations to nonexistent fixtures.

The campaign runs in three env-gated modes controlled by `FUZZ_SHARE_HEADERS`, `FUZZ_MARKERS`, `FUZZ_READONLY`. The first two default on (`fuzz.sh run` sets them); `FUZZ_READONLY` stays off unless exported and the target instance has read-only mode enabled.

## Per-role runner

    just fuzz roles

Runs one deterministic pass per role (admin0, admin1, memberA, memberB,
sharingDisabled, disabledUser, shareGuest, snapshotBearer, anonymous) with
identical budget, writing `report-<role>.json` into `utils/fuzz/out/`
(override with `--report-dir`). `FUZZ_STATE_DIR` reuses a seeded state
directory instead of a throwaway one; `--state-dir` is the flag form.

The role hooks (`hooks_roles.py`) inject per-role credentials (session cookie,
a runtime-minted snapshot bearer, share-guest headers with an optional share
password, or no auth for the anonymous pass), refresh an invalidated
credential with a single re-login/re-prefetch on a 401 and replay the request
once (attempt/success/failure counters are printed at run end; disabled
credentials exhaust after one failed attempt so refresh never loops), and
apply poison guards that null config fields and redirect logout, password,
admin, disable, sharing, delete, index rebuild, read-only toggle and
signing-key reset operations to nonexistent fixtures.

A rate/limit response aborts the whole run: it invalidates the cross-role
comparison rather than being a finding.

## Differential diff

    just fuzz diff

Compares the per-role reports produced by `just fuzz roles` and fails on NEW
cross-role diffs only. Uses `matrix.json` (per-(role,operation) expected
class) and `baseline-roles.json` (triaged diffs). The Python entry point takes
`--report-dir`, `--baseline`, and `--matrix` arguments
(`python3 utils/fuzz/diff.py --help`).

## Harness entry point

`utils/fuzz/fuzz.sh <verb> [options] [schemathesis args...]` is the
single entry point the recipes call. `check` and `run` forward unknown
arguments to Schemathesis; `roles` accepts only its documented options so a
per-role comparison stays deterministic. `diff` forwards its arguments to
`diff.py`.

Internal pieces, used by the entry point or by tests:

    utils/fuzz/seed.sh              # seed a state dir (FUZZ_STATE_DIR)
    utils/fuzz/check_report.py      # baseline compare of one report
    utils/fuzz/fuzz_matrix.py       # matrix checks (seeded env)
    ./scripts/schemathesis-run.sh <version> <schema> <url> [args…]  # raw uvx passthrough, no harness config

## Auth knowledge derivation (`backend/auth-matrix.json`)

The hooks do not carry hand lists of auth knowledge. `auth_rules.py` derives it from `backend/auth-matrix.json` (the committed machine contract of the per-operation `x-picasu-auth` annotation, regenerated and diffed by `openapi-check`):

- share-header injection set — the `GuardPresenter`-guarded operations (both tiers)
- check assignment — which operation carries which of the cross-scope / wrong-credential / renewal-mismatch / demoted checks (`test_plan`, one check per operation by priority)
- wrong-credential token selection — a token outside the operation's `credentials`
- read-only exemptions — non-GET operations without `GuardMutate`
- scope pairs — the seeded share identities crossed with the foreign albums they must not reach

The derivation is pure and unit-tested; `just utils-test` runs the suite (or `python3 utils/fuzz/test_auth_rules.py` for the file alone), so CI and the precommit hook enforce it. The tests pin the derivations against the committed matrix, so a backend guard change fails them and forces a check review instead of silent drift. A missing or malformed matrix file fails the run (override the location with `FUZZ_AUTH_MATRIX`). The destructive-operation disables in `campaign.toml`/`check.toml` stay hand-tuned, with a unit test keeping their operation ids in sync with the matrix.

What A1-A5 assert (probe targets are generated from the matrix, not hand-picked):

- A1 `cross_scope_refused` — a share-A snapshot token cannot read an album-B asset: every snapshot-typed operation whose path binds an asset id answers 404 on the mismatch (S4 binding semantics — not 403, so the route does not reveal that the id exists).
- A2 `demoted_token_rejected` — a demoted user's still-live token loses admin access immediately: every admin-guarded session operation answers 401.
- A3 `wrong_credential_rejected` — a session-typed token does not satisfy a snapshot-token guard: every snapshot-typed bearer operation answers 401.
- A4 `readonly_405` — every mutating (`GuardMutate`) operation answers 405 under read-only mode (env-gated via `FUZZ_READONLY=1`; requires read-only mode enabled on the target instance).
- A5 `cross_share_renewal_refused` — share B cannot renew a snapshot minted for share A: the album-scoped renewal operations (AUTH-10) answer 401.

Stateful phase uses up to 8 steps per sequence.

## SARIF / Security tab

`utils/fuzz/json2sarif.py <report.json> <baseline.yaml> <out.sarif>` converts a run report to SARIF 2.1.0. Baseline-accepted findings are excluded; results are advisory (`level: warning`). Operation anchoring is line-based against `backend/openapi.json`. The nightly workflow uploads the result under the `schemathesis` category.
