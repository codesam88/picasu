---
status: done
type: feature
priority: medium
area: tooling
---

## Notes

Scheduled, non-blocking MSRV floor check. Floor (1.90) is the maximum
`rust-version` across the locked dependency tree, verified by reading
`cargo metadata`; our own code needed two `Duration::from_hours`
(stabilized 1.91) call sites lowered to `from_secs` to meet it.

## Goal

`just msrv-check` installs the floor toolchain and runs
`cargo check --workspace --locked --all-targets` on it.
`.github/workflows/msrv.yml` runs it on schedule + manual dispatch only;
a red run means code or locked deps moved above the floor.
