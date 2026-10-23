---
status: done
type: chore
priority: medium
area: devops
---

# License / SPDX review (pre01 C1)

Pre-first-release gate: dependency licenses and SPDX metadata. The OpenSSF
Scorecard was evaluated and dropped (see Decisions); it is not a gate.

## Done (verified 2026-10-09)

- **License / SPDX:** root `LICENSE` (MIT), README license section,
  `license = "MIT"` in `backend/Cargo.toml`, `frontend/package.json` is
  `"private": true` (not published, correctly unlicensed). `cargo deny check`
  runs green: `advisories ok, bans ok, licenses ok, sources ok`.
  `npm audit --omit=dev` finds 0 vulnerabilities.
- **Scorecard Security-Policy:** `SECURITY.md` landed on main (`fc3ff60a`).
  Kept for its own value; the score is no longer tracked.

## Remaining

Tag time: decide release signing before the first tag (`release.yml` does not
sign). Branch protection stays as-is (single maintainer, direct pushes to
main); no scorecard score is chased.

## Not pursued

- Scorecard `Vulnerabilities`: same OSV data `cargo deny` / `npm audit` /
  Dependabot already gate on, weekly cadence, no PR gating, and a second
  waiver syntax to maintain. Workflow (`.github/workflows/scorecard.yml`) and
  both `osv-scanner.toml` waivers deleted 2026-10-23.
- `Code-Review`, `Contributors`, `Fuzzing` — structural to a
  single-maintainer repo without fuzz targets.

## Decisions

- 2026-10-09: quick-xml advisories — wait for xmpkit 0.1.7 instead of
  vendoring; paste-shim stays until pulp and rav1e release their pastey
  migrations. Removal steps tracked in `pending-upstream-dep-releases.md`.
- 2026-10-23: drop the Scorecard workflow and `osv-scanner.toml` waivers —
  duplicate audit mechanism with no coverage beyond `cargo deny` / `npm audit`
  / Dependabot except dev-only tooling.
