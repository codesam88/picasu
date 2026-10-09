---
status: done
type: chore
priority: medium
area: devops
---

# License / SPDX / OSSF scorecard review (pre01 C1)

Pre-first-release gate: dependency licenses, SPDX metadata, and the OpenSSF
Scorecard. Findings below; status flips to `done` once the merged scorecard run
confirms the remaining fix.

## Done (verified 2026-10-09)

- **License / SPDX:** root `LICENSE` (MIT), README license section,
  `license = "MIT"` in `backend/Cargo.toml`, `frontend/package.json` is
  `"private": true` (not published, correctly unlicensed). `cargo deny check`
  runs green: `advisories ok, bans ok, licenses ok, sources ok`.
  `npm audit --omit=dev` finds 0 vulnerabilities.
- **Scorecard Security-Policy:** `SECURITY.md` landed on main (`fc3ff60a`);
  check went 0 → 10, total 6.6 → 7.1 (run 2026-10-09T16:28Z).
- **Scorecard Vulnerabilities (5 found):** waived via `osv-scanner.toml` at the
  repo root and in `frontend/` (commit `8fa6d85d` on `fix/scorecard`) — the
  mechanism the scorecard documents for ignores. Validated locally with
  osv-scanner v2.6.0: all 5 filtered, both scans "No issues found".
  Reasons mirror `deny.toml`; see also `pending-upstream-dep-releases.md` for
  the two waits that will delete waivers outright.

## Remaining

1. Merge `fix/scorecard` and confirm the next scorecard run clears
   `Vulnerabilities`. Docs say the check honors `osv-scanner.toml`; treat a
   still-failing check as a finding, not a no-op.
2. `Branch-Protection` is 3: ruleset 18236662 only enforces `deletion` +
   `non_fast_forward`. Adding `pull_request` with
   `required_approving_review_count: 1` would roughly double it but ends direct
   pushes to main — maintainer's call, offered 2026-10-09, unanswered.
3. Tag time: `Packaging/Signed-Releases` is -1 (excluded from scoring while no
   release exists); `release.yml` does not sign. Decide signing before the
   first tag.

## Accepted as structural (not chased)

- `Code-Review: 0` and `Contributors: 0` — single-maintainer repo.
- `Fuzzing: 0` — no fuzz targets; revisit post-0.1 if warranted.
- `SCORECARD_TOKEN` unset — per scorecard docs it can only make results more
  accurate, never raise the score.

## Decisions

- 2026-10-09: quick-xml advisories — wait for xmpkit 0.1.7 instead of
  vendoring; paste-shim stays until pulp and rav1e release their pastey
  migrations. Removal steps tracked in `pending-upstream-dep-releases.md`.
