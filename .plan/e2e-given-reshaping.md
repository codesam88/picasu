---
status: open
type: feature
priority: medium
area: full-stack
---

# Reshape the e2e scenario harness: pre-launch givens, store-level

# provisioning, declared startup options, explicit session/index steps

Successor design to the band-aid in `.plan/e2e-boot-guard-scenario-red.md`
(harness enroll+login with the `session: none` directive), captured from
the 2026-10-18/19 design discussion. The band-aid keeps the suite green
in the meantime; this task replaces it with the declaration-first shape.

## Problem with the current harness (verified)

1. **API acrobatics for state.** `given: user:` rides
   `POST /post/users/bootstrap`, which only works inside the bootstrap
   window (launcher sets `PICASU_BOOTSTRAP_ADMIN=true` unconditionally)
   and only for the _first_ user. Since the ephemeral open-mode mint was
   removed, the HTTP API on an empty running store offers nothing else:
   logins answer the uniform unknown-user 401 and guarded writes refuse,
   so every online seeding must enroll first. The scaling path is
   offline provisioning, which the `picasu-cmd` CLI provides (shipped in
   `.plan/user-store-redb.md`): it seeds user records before boot, the
   store is read live at request time, and `/post/users/authenticate`
   then works normally. Interplay: the harness cannot keep forcing the
   flag — flag on against
   a non-empty user store refuses to boot (`lib.rs`), so the CLI path
   pairs with dropping the unconditional launcher flag (or flagging only
   genuinely empty installs). None of the bootstrap riding scales to the
   planned "create more users and switch between them" scenarios.
2. **A documented tool that doesn't exist.** Error messages and
   `docs/openapi-reference.md` promise a `picasu-cmd` CLI for store
   provisioning; the binary has one clap surface with no subcommands
   (`main.rs`).
3. **Watchers fire on every launch.** The backend fixture starts first
   (default watcher on), then `given:` writes every photo into the
   watched tree — N debounced (1 s) `index_image` tasks racing the
   explicit index. Incidental, noisy, and not what the tests rely on:
   indexing already runs explicitly (`POST /post/index/album` +
   `/get/index/status` poll inside `executeGiven`).
4. **Implicit index by heuristic.** The hidden given-phase index runs
   only when `seedEntries.length > 0`; scan-flow tests depend on the
   _absence_ of that heuristic (`photo_raw` comment), and
   `covers.api` credits index calls to scenarios via given-tracing
   whether or not they intend it.
5. **Session policy as a special directive.** The band-aid's
   `session: none` key encodes today's coincidence (self-managed
   scenarios happen to declare `user:`) rather than a scalable
   semantic.

## Settled design (user decisions, 2026-10-19)

### Phase model

| Phase                                                       | Contents                                                                                                   | Needs server |
| ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ------------ |
| A — pre-launch (after `createPaths`, before `startBackend`) | snapfab generation (`dir_album`, `photo`, `photo_raw`, `source_file`), user injection into the real stores | no           |
| B — launch                                                  | env from the scenario's `server:` declaration                                                              | —            |
| C — pre-steps                                               | `config:`/`auth_key:` given via API (real auth against injected users), scenario-driven index step         | yes          |
| D — steps                                                   | `login:` + scenario body                                                                                   | yes          |

### Store-level user provisioning (replaces the HTTP bootstrap path)

- Implement the promised **`picasu-cmd` provision command** (shipped:
  `.plan/user-store-redb.md` — `user add|mod|del|passwd|list` on the
  `users.redb` userdb of record). Offline and strictly so: the store's
  engine lock is held by a running server, so the CLI must run **before**
  `startBackend` and fails loudly if the server holds the store.
- `given: user:` keeps its declaration shape; its `executeGiven` branch
  stops calling HTTP and instead feeds declarations to the launcher.
- Bootstrap-window usage becomes an explicit, separate declaration
  (name TBD: e.g. `user_via_bootstrap:`) for the few scenarios that
  genuinely exercise `POST /post/users/bootstrap` and window semantics.
- Fail-fast at load: `server.bootstrap_admin: true` + injected users =
  guaranteed boot refusal (`lib.rs:81`) → clear harness error, not a
  launcher timeout.

### Step B — declared startup options

```yaml
server: # optional; defaults
  bootstrap_admin: true # default false (injected passwd makes true+injected a boot refusal)
  pbkdf_iterations: 1000 # default 1000
```

Typed zod subset + `schema.json` mirror (no free-form env passthrough);
`startBackend` maps it onto the env it builds (`extraEnv` remains for
`.spec.ts` specs). Launcher's hardcoded `PICASU_BOOTSTRAP_ADMIN=true`
becomes declaration-driven; `PICASU_PBKDF_ITERATIONS` keeps its 1000
default, overridable per scenario.

### Step C — index owned by the scenario

New step verb `index: "/"` — `POST /post/index/album` + poll
`/get/index/status` to done (poll loop lifted from `executeGiven`).
Scenario files that assert indexed media declare it; scan-flow
scenarios omit it deliberately (absence becomes visible instead of a
`seedEntries` heuristic). Coverage becomes honest: the call is made
from the step, credited to the scenario that declares it. The
`seedEntries.length > 0` conditional in `executeGiven` is deleted.

### Session — explicit steps, no special directive

- New step verb `login: { user: admin }` (password from the declared
  `user:` given, or inline) — one interpreter implementation, UI
  coupling in one place, multi-user ready (extra `user:` given + a
  `login:` step per switch).
- `session: none` and the harness auto-login/auto-enroll from the
  band-aid are **removed**; every scenario states its own preconditions
  (`user:` given + `login:` step + `index:` step as needed).
- `postIndexMoves`/`move:`/`remove:` givens: ordering constraint is
  "after index", not "before launch" — zero consumers today; becomes a
  step verb when a scenario first needs it.

## Work split (iterative, worker-delegated, TDD where applicable)

- **H1a** — backend `picasu-cmd` provisioning command + tests
  (delivered: `.plan/user-store-redb.md`).
- **H1b** — harness plumbing: scenario known before backend start
  (`test.use` in the spec loop or backend start in the body), phase-A
  seeding (users via CLI, snapfab), `server:` declaration +
  fail-fast validation, conditional bootstrap flag.
- **H1c** — DSL verbs `login:` + `index:` (types/schema/docs), purge
  the given-phase index + `seedEntries` heuristic.
- **H1d** — scenario YAML rework: drop `session: none` and harness
  auto-login; add `user:`/`server:`/`login:`/`index:` declarations;
  explicit `user_via_bootstrap:` where window semantics are the subject.
- **H1e** — gate: full `just frontend-playwright` green, coverage
  warning counts stable, `just frontend-check`, DSL docs sync
  (`docs/playwright_generator.md`, `schema.json`).

## Verification

Full Playwright suite green with no harness special directives left;
coverage warnings not worse than the band-aid baseline; boot-refusal
fail-fast covered by a harness-level test or documented manual check.

## Notes

- `first-admin-registration.spec.ts` and `login-throttle.spec.ts` are
  code specs on `startBackend({ extraEnv })` — unaffected except where
  they opt into the new declaration surface later.
- The launcher's unconditional bootstrap flag is also load-bearing for
  the band-aid today (auto-enroll via API bootstrap) — H1b must
  sequence the flag flip after CLI provisioning lands.
