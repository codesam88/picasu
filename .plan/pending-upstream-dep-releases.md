---
status: backlog
type: chore
priority: medium
area: devops
---

## Notes

Two dependency shims/waivers exist only because their fixes are merged upstream
but unreleased. When the releases land, remove the shims and their waivers in
the same change. Both waits are external; check upstream before starting.

### 1. quick-xml >= 0.41 (RUSTSEC-2026-0194 / -0195)

- Blocker: xmpkit pins `quick-xml ^0.40`; published latest is 0.1.6.
  Upstream main already requires 0.41 (dependabot PR to 0.42 open); release
  tracked in cavivie/xmpkit issue #138 ("chore: release v0.1.7").
- Verified 2026-10-09: xmpkit 0.1.6 compiles against quick-xml 0.41.0 with
  zero source changes (probe in /tmp), so 0.1.7 should be drop-in. 0.41.0's
  changelog contains only the two advisory fixes + one new feature.
- Reachability note: 0194 is reachable (indexer `extract_xmp_data_from_file`,
  plus read-modify-write re-parse on edit endpoints); 0195 is not (xmpkit
  never uses `NsReader`). Do not vendor xmpkit — user chose to wait.
- On release: `cargo update -p xmpkit -p quick-xml`, then delete the
  RUSTSEC-2026-0194/0195 entries from root `osv-scanner.toml` and the matching
  ignore entries + comments from root `deny.toml`.

### 2. paste-shim removal (RUSTSEC-2024-0436)

- Requires BOTH releasers, since either alone keeps `paste` in the tree:
  - pulp: PR #39 "chore: switch to pastey" merged 2026-09-08, unreleased
    (latest 0.22.3 still has `paste ^1`).
  - rav1e: main has `pastey = "0.1.0"`, unreleased (latest 0.8.1, 2025-06-16,
    still has `paste ^1.0`).
- On release: `cargo update -p pulp -p rav1e` (confirm `paste` left
  `Cargo.lock`), delete `utils/paste-shim/`, the root
  `[patch.crates-io] paste` line, and the RUSTSEC-2024-0436 entry from root
  `osv-scanner.toml`.
- Note: the shim does not hide the advisory from OSV (name+version match);
  its value is compiling maintained `pastey`. deny.toml has no 2024-0436
  entry — it was removed when the shim landed; do not re-add one.

### Checks for either change

`cargo deny check advisories`, `osv-scanner scan` on both lockfiles (root
Cargo.lock + frontend/package-lock.json), `just check`, `just test`. When both
tracks are done, root `osv-scanner.toml` should contain only RUSTSEC-2026-0258
(h2).
