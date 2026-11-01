---
status: backlog
type: chore
priority: low
area: repo
---

## Notes

Test output must survive the run that produced it: a timeout or a piped
`tail` discards the failure names, and the re-run costs more than the fix
(e.g. one 18-minute `cargo test --lib` re-run to recover 9 failure names
lost to `| tail -n 4`).

## Problem / purpose

Two different log-loss modes, one shared fix shape (persist full output,
print the path on failure):

- Agent-side piping (`| tail`) drops failure lists from long suites.
  Already handled by discipline: redirect long runs to
  `/tmp/opencode/<name>.log` and only then summarize.
- Playwright timeouts/failures leave nothing behind: `just
frontend-playwright` runs `npx playwright test` with default
  reporters, so a timed-out run's evidence exists only in scrolled
  terminal output.

## Solution strategy

Do NOT hardcode the agent temp dir (`/tmp/opencode`) in repo recipes —
it is environment-specific. Prefer tool-native persistence:

- Cargo side needs no repo change: `cargo nextest run` (what `just
backend-test` uses) already archives per-test output; the loss was
  agent piping, fixed by the redirect discipline above.
- Playwright side is the real gap: configure a persistent reporter
  (json and/or html to a stable repo-local path, gitignored) and echo
  the report path when the run fails or times out.

Rejected: tee-ing every just recipe to a log dir — repo-local logs
need gitignore + rotation policy for a problem nextest already solves.

## Implementation steps

1. Add a Playwright reporter (json, plus html if cheap) writing under a
   stable gitignored path; verify a failing run leaves the report.
2. Echo the report path on failure/timeout in `frontend-playwright`
   (and document the path in `docs/playwright_generator.md`).
3. Leave cargo recipes alone; record the `/tmp/opencode/*.log`
   redirect as agent-runner convention, not repo mechanism.

## Explicitly out of scope

- Changing what the suites assert; log rotation/retention policy
  beyond a gitignored directory.
