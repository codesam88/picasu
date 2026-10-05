---
status: open
type: bug
priority: high
area: backend
---

## Notes

Read-only mode cannot be lifted through the API. `PUT /put/config`
(`backend/src/router/put/edit_config.rs:40-51`) takes
`read_only: GuardResult<GuardReadOnlyMode>` and propagates it
(`let _ = read_only?;`), and the same request assigns
`current_config.read_only_mode` (`edit_config.rs:86-87`). So the endpoint that
would set the mode to `false` is refused with **405 Method Not Allowed** the
moment the mode is on. Lifting it today requires a restart with
`PICASU_READ_ONLY_MODE=false` (`backend/src/model/config.rs:403-408`) or a config
file edit.

The settings UI exposes the toggle (`frontend/src/components/Page/Config/AdvancedConfig.vue:10`,
submitted through the ordinary config update path), so the switch can be turned
on and then cannot be turned off — a user-facing dead end, not only an API
wart. No test covers the API direction: the suites toggle the setting by writing
config directly (`backend/src/tests/backend_api.rs:852`) and reset it the same
way (`backend_api.rs:1041`), which is why the lockout was never exercised.

### The fix, decided 2026-09-28

**A new guard for explicit re-authentication**, used by a route that disables
read-only mode. A valid token is not enough; the caller must present the account
password again. Lifting a server-wide restriction is a privilege escalation
step, so a captured or left-behind token must not be sufficient on its own.

It is a guard rather than a password field in this request because the same
requirement belongs to a second operation — **changing the user password**
(`update_password_handler`, `edit_config.rs:139-147`) — and because the
requirement is a property of the operation, not of one handler's payload. It
lives beside the other guards in `backend/src/router/auth.rs`, and moves the
password comparison out of `update_password_handler` into the guard, so both
operations get the same behaviour and the same error.

How the credential arrives is a design decision the guard settles rather than
each caller: a request header (say `X-Picasu-Password`) keeps it out of request
bodies and out of logs, which is the reason to prefer it. `update_password_handler`
reads it from a JSON field today (`old_password`), and that field stays for
compatibility with the existing client while the header is what new callers
should send.

### Scope

- **A route that mutates config while read-only mode is on, so it must not carry
  `GuardReadOnlyMode`.** That makes it the single named exception to the
  "every mutating route carries the mode guard" rule (M1) in
  `.plan/openapi-annotation-checks.md`, and this plan is where that exception is
  justified. The route still requires `GuardAuth`.
- **The guard compares the password in constant time.** `update_password_handler`
  uses `!=` (`edit_config.rs:157`), which is fine for a local value and wrong for
  a credential check; the new guard must not inherit the habit.
- **`PUT /put/config` keeps its guard** and must document the 405 it can now
  return, which it does not today — the operation lists 200/400/401 only. That is
  the rule M2 in the annotation-checks plan, and this is where its first finding
  gets fixed.
- **The frontend toggle has to move**: it must call the new endpoint and prompt
  for the password. Otherwise the switch stays stuck even after the backend is
  fixed.
- **Documentation**: `docs/openapi-generator.md` for the route-set and mode
  story; the new operation's annotation declares its 401s (no token, wrong
  password) and its success response.

### Behaviour to decide (open questions)

1. **Route shape**: a new operation (`PUT /put/config/read-only-mode`, or
   `POST …/disable`), or a dedicated field on `PUT /put/config` that is exempt
   from the guard. The first is auditable in the document as its own operation;
   the second keeps one endpoint but makes the guard conditional.
2. **Status for a wrong or missing password**: 400 (as
   `update_password_handler` does today) or 403. 403 is more honest about _why_.
3. **No password configured.** `AppConfig::password` defaults to `None`
   (`config.rs:113`), so a fresh install has none and the guard could never be
   satisfied — the lockout would persist for exactly the installs least likely
   to have a password. Either require the config `auth_key` as the fallback
   credential, or fail closed with an error naming the restart path. This needs
   an answer before implementation, and with a guard it is answered once for both
   call sites rather than per endpoint.
4. **Scope of the re-authentication**: only for lifting the mode, or also for
   setting it? The decision covers disabling; enabling is harmless by
   comparison, so the asymmetry is intended.

### Acceptance

Tests first, each one a named case rather than a sweep:

1. With read-only mode **on**, `PUT /put/config` carrying `readOnlyMode: false`
   is refused with 405 — the remaining behaviour is pinned deliberately, so a
   future change to it has to be a decision.
2. The new endpoint without a token → 401.
3. With a token and a wrong or missing password → the status chosen above.
4. With a token and the correct password → 200, `read_only_mode` is false
   afterwards, and the change survives a config reload.
5. The new route's signature carries `GuardAuth` and no `GuardReadOnlyMode`.
6. The case from open question 3 (no password configured) behaves as decided.
7. `just openapi-gen` produces a reviewed diff, `just openapi-check` is green,
   and the frontend toggle is exercised by a scenario if the existing suite can
   reach it — otherwise say so rather than claiming coverage.

### Progress: 2026-10-04 — premise corrected, scope widened

The claim above that lifting read-only "requires a restart ... or a config file
edit" is **wrong**: `POST /post/config/import` (`post/import_config.rs`) takes
only `GuardAuth` and replaces the whole `AppConfig` through
`AppConfig::update`, including `read_only_mode`. Verified against a running
instance: with the mode on, `PUT /put/config` correctly answered 405 while a
single `POST /post/config/import` carrying `"readOnlyMode": false` answered 200
and mutations were accepted again.

Consequence for the decided fix: the re-auth guard must cover **import_config**
too, not only a new disable endpoint — otherwise a captured admin token defeats
the re-authentication requirement through the side door. Recorded as finding F9
in `docs/auth.md`.
