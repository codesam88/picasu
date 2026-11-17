# Brief: Fuzzing an API with multiple user accounts and roles

Researched: 2026-11-17 | Versions: Schemathesis 4.30.x (4.24 reactive-refresh, 4.26 WFC multi-user fixes), EvoMaster (2025-2026 access-policy oracles), RESTler-fuzzer (2024-2026), WFC auth spec, OWASP ASTF | Overall confidence: high for Schemathesis mechanisms (primary docs + source); medium for cross-tool oracles (convergent secondary sources)

## 1. Baseline

Goal: exercise the same API as several principals (e.g. admin, member, guest, anonymous) so the fuzzer finds broken access control, not just crashes.

Standard vocabulary:

- **Principal / identity**: a credential set the fuzzer presents (cookie, header, bearer token). Roles are admin, authenticated non-admin, delegated guest (share/snapshot), anonymous.
- **Horizontal vs vertical**: horizontal = same role, different owner (can Alice read Bob's object?); vertical = low role vs high role (can member call admin ops?); tenant = same role, different scope.
- **Oracle**: the rule that decides pass/fail. Two families: status-code oracle (401/403 expected vs 2xx) and differential oracle (same request with two credentials must not both succeed the same way).
- **Stateful fuzzing**: the fuzzer chains calls (create → read → update → delete) in scenarios; auth state can change mid-scenario (password change, disable, key rotation, expiry).
- **Poison guard**: requests the harness must never send with real credentials (logout, delete-self, demote-last-admin, rotate signing key) because they destroy the session under test.

Baseline pattern that works with any fuzzer: seed one account per role out-of-band, run the fuzzer once per identity (or rotate identities per request), and compare outcomes for the same operation across identities. Anonymous gets its own pass with no credentials at all.

## 2. What research adds

### Corrections and updates

- **Static tokens are a seed, not a session strategy.** Common belief: putting a JWT in config headers is "authenticated fuzzing". Current Schemathesis (4.x) treats that as one cache entry that goes stale; the supported pattern is a token-fetching provider with proactive (`refresh_interval`, default 300 s) and reactive (`retry_on: [401]`, single refetch + replay) refresh. If the provider cannot re-login, a mid-run 401 poisons the rest of the run. [schemathesis-docs-auth; schemathesis-source-auths.py]
- **401 retry is bounded and pooled.** Belief: each scheme retries independently. Actually `retry_on` codes are pooled across all configured schemes, raw `call()` does not replay, only `call_and_validate` / state-machine / `run` / `fuzz` do, and 3 consecutive fetch failures disable refresh for the run (30 s cooldown). Design hooks to surface "refresh failed" loudly instead of silently continuing unauthenticated. [schemathesis-docs-config; schemathesis-source-auths.py]
- **WFC escalation is ordered, not weighted.** Belief: listing N users gives proportional coverage. Actually the escalating provider tries first user → anonymous → remaining users per operation until first 2xx/3xx, then locks that identity (`record_auth_outcome`, `snapshot`/`restore`). There is no native traffic-weight knob; proportional splits (e.g. 20/80) must be built in hooks or by separate per-role runs. [schemathesis-source-escalation.py; schemathesis-docs-config]

### Beyond the baseline: practices and constraints

- **Declare users out-of-band (WFC pattern).** Keep the account list (`auth: [{name, fixedHeaders} | {loginEndpointAuth}]` with `payload`, `extractFrom`, `expectCookies`) outside the OpenAPI schema; the schema only declares security schemes. Rationale: short-lived tokens do not belong in a committed spec. [wfc-auth-spec; wfd-paper]
- **Prefer per-role runs feeding a differential harness over in-run coin flips.** Run the same operation set as admin, as member, as guest, as anonymous (deterministic, reproducible with fixed seed), then diff (status, headers, body shape, field inventory, side effects). In-run random rotation is non-reproducible and hides which identity found what. [evomaster-access-policy-paper; owasp-astf; trusteed-response-diff]
- **Use scope-aware token caches.** When one provider serves several roles, key the cache by scope derived from the operation's `security` stanza (e.g. `cache_by_key = fn(case, ctx)`), giving one cache entry per role/scheme instead of one global token. Rationale: prevents a member token overwriting the admin entry. [schemathesis-docs-auth; qaskills-schemathesis-auth]
- **Track principal identity in scenario state, not in payloads.** In state-machine mode (`setup` logs in per scenario, `before_call` attaches headers from scenario state, `teardown` cleans up with the owner's credential). Rationale: payload-derived IDs get mutated by the fuzzer; scenario state does not. [schemathesis-docs-stateful; qaskills-schemathesis-auth]
- **Seed-state-at-start, never mid-run.** Create accounts, shares, and a minimum object corpus (e.g. one image per album/scope) before the run; fail closed if seeding did not produce rows. Rationale: object/asset endpoints have no valid target with zero rows, and mid-run creation races with guards. [schemathesis-docs-stateful; mayhem-auth-blog]
- **Exclude the auth flow from the fuzz grammar.** Never fuzz the login/logout/rotate-key/delete-user endpoints with real credentials in the same run; test them in a separate short auth-only campaign, optionally unauthenticated. Rationale: they invalidate the session under test (RESTler refuses to fuzz auth; Mayhem uses `--ignore-endpoint`; GitLab uses exclude-paths). [restler-auth-docs; mayhem-auth-blog]

### Pitfalls and failure modes

- **Admin false positives.** A global "users must not touch each other's resources" rule fires on admin/moderator/delegated-guest paths that legitimately cross owners. Mitigation: per-(role, object, action) expectations in an authorization matrix; per-endpoint oracle disable after triage. [evomaster-access-policy-paper]
- **OR-scheme policies need per-endpoint expectations.** Operations accepting session OR share OR bearer return 2xx under several identities; asserting "non-admin must 4xx" misfires. Assert 2xx-with-wrong-scope only where a probe explicitly covers it. `lead` [eresus-bola-guide; trusteed-response-diff]
- **Full-body comparison is noise.** Ordering, timestamps, session IDs differ per request. Use marker matching plus order-independent record comparison. [mysecscan-tools; trusteed-response-diff]
- **Same-tenant accounts look like BOLA.** Two accounts in one org are supposed to share data; use genuinely separate tenants/scopes for horizontal tests. [mysecscan-tools]
- **Expired credentials masquerade as authz faults.** Verify credential validity before flagging 401s; check the auth info first. [evomaster-access-policy-paper]
- **Catch-all / soft-404 baselines.** Probe nonexistent paths first so guessed paths do not look like real endpoints. [owasp-astf]
- **Body-level success on 200.** Some APIs return 200 with `{"status":"fail"}`; oracles must inspect body markers, not just status. [owasp-astf]
- **Side effects beyond status.** A denied mutation that still enqueues a job or writes a row is a failure even with 403. Assert row/queue/cache counts where the harness can. [eresus-bola-guide; owasp-astf]
- **Uniform 404 vs mixed 403/404.** If "not found" and "not yours" return different shapes/timings, existence leaks; mixed conventions across endpoints are themselves a finding. [trusteed-response-diff; eresus-bola-guide]
- **Shared caches keyed without principal.** A controller check can pass while a cache returns another user's object. Include cache keys in the oracle where applicable. `lead` [trusteed-response-diff]
- **Token-independent operations defeat re-auth loops.** If replay still returns a `retry_on` status, Schemathesis stops refreshing that operation; track and report rather than looping. [schemathesis-source-auths.py]

### Alternatives and trade-offs, including when NOT to

| Approach                                                                          | Use when                                                                         | Avoid when                                                                                     |
| --------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Separate deterministic run per role + differential diff                           | Highest signal for access control; reproducible; fits CI per-role coverage gates | Finding crashes fast matters more than who can call what (use single admin run)                |
| Peer-replay (Burp Autorize / Schemathesis `peers` + `object_level_authorization`) | Read-only cross-tenant BOLA on GETs with two real accounts                       | Writes must be replayed (peer check is read-only by design)                                    |
| WFC escalation (ordered try-until-2xx)                                            | Quickly mapping "which identity can call this op"                                | Proportional load (20/80) or proving a specific role is denied (locks on first success)        |
| Custom `before_call` rotation with deterministic key                              | Proportional attention per role with fixed-seed reproducibility                  | Team cannot maintain per-(role,op) expectations; false positives from OR schemes untriaged     |
| `ignored_auth`-style unauthenticated probes only                                  | Cheap missing-auth gate on declared-auth ops                                     | Delegated/snapshot paths where "no credential" is not the only negative (need wrong-scope too) |

### Recommended approach for this context

1. Seed N named accounts plus delegated-guest artifacts out-of-band (login flow with `extractFrom` cookie/body, `expectCookies` where applicable); persist one credential per role to files the hooks read; fail closed if any role has zero rows/objects.
2. Run one deterministic pass per role with fixed seed (admin small, e.g. ~20%; member/guest/snapshot/anonymous the rest), not one run with random rotation. Keep destructive/auth-mutating ops excluded or poison-guarded with real IDs rewritten to nonexistent fixtures.
3. Enable reactive refresh (`retry_on` covering the app's unauthenticated statuses) with a visible counter; refresh must re-run the declared login flow, not replay a static header.
4. Diff the same operation across roles (status + body-shape + side effects) against a checked-in authorization matrix; per-endpoint overrides live in the matrix, not in hook `if` chains. Triage once, then gate.
5. Keep a dedicated anonymous pass (no credentials, invalid-credential variant) as the missing-auth gate; handle multi-scheme (OR) operations as explicit matrix rows, not global rules.

## 3. Worker checklist

Do:

- [ ] Confirm how the fuzzer under test declares login (endpoint, payload, `extractFrom` body/header/cookie, `expectCookies`) against the app's actual auth code, not just the spec.
- [ ] Seed one account per role plus one object per scope before the run; assert non-empty per role; abort the run if seeding failed.
- [ ] Exclude or rewrite-with-nonexistent-ID: logout, password change, role change, disable/delete, signing-key rotation, share/token redemption in generative traffic.
- [ ] Implement refresh-on-unauthenticated (refetch + single replay) with attempt/success/failure counters printed at run end; verify manually by forcing a mid-run invalidation in a staging run.
- [ ] Run per-role passes with fixed seed; record per-(role, operation) status histograms; diff across roles for the same operation ID.
- [ ] Maintain a checked-in authorization matrix; any new 2xx-with-unexpected-role fails the run until triaged into the matrix.

Don't:

- [ ] Don't fuzz with a single static header and call it authenticated coverage.
- [ ] Don't use random per-request identity without a deterministic key and per-role counters.
- [ ] Don't assert "non-admin must 4xx" globally; OR/delegated paths need per-endpoint rows.
- [ ] Don't compare full bodies byte-for-byte across roles; compare shapes/markers/side effects.
- [ ] Don't create the seed data inside generative scenarios; race with guards causes flakes.

Validate by: per-role call counts within the planned band; forced-invalidation staging run recovers via refresh (or fails loudly, never silently unauthenticated); differential diff on a known-good build is empty; matrix regeneration passes.

## 4. Sources

- Schemathesis auth guide (primary docs) — dynamic providers, `cache_by_key`, `apply_to`/`skip_for`, WFC escalation, retry_on. https://schemathesis.readthedocs.io/en/latest/guides/auth/
- Schemathesis configuration reference (primary docs) — `auth.openapi`, `auth.dynamic`, `auth.wfc`, `retry_on`, `request-retries`. https://schemathesis.readthedocs.io/en/latest/reference/configuration/
- Schemathesis `auths.py` source (exemplar code) — provider protocol, caching/keyed providers, reauth breaker. https://github.com/schemathesis/schemathesis/blob/master/src/schemathesis/auths.py
- Schemathesis `wfc/escalation.py` source (exemplar code) — ordered escalation, peers, snapshot/restore. https://github.com/schemathesis/schemathesis/blob/master/src/schemathesis/wfc/escalation.py
- Schemathesis stateful-testing guide (primary docs) — setup/teardown, `before_call`, scenario-scoped auth. https://schemathesis.readthedocs.io/en/latest/guides/stateful-testing/
- Schemathesis hooks reference (primary docs) — `before_call`/`after_call` signatures and phase scoping. https://schemathesis.readthedocs.io/en/latest/reference/hooks/
- WFC auth spec (spec) — out-of-band named users, `loginEndpointAuth`, `expectCookies`, peers. https://github.com/WebFuzzing/Commons/blob/master/auth.md
- EvoMaster auth docs (primary docs) — multi-`AuthenticationDto`, declarative login TOML. https://github.com/WebFuzzing/EvoMaster/blob/master/docs/auth.md
- Sahin, Zhang & Arcuri (2026), Enhancing REST API Fuzzing with Access Policy Violation Checks (paper) — multi-user oracles, admin false-positive handling, 403→2xx oracle. https://arxiv.org/html/2604.00702v1
- OWASP API Security Testing Framework, TESTING_GUIDELINES (methodology) — `--secondary-token` cross-user method, baselines, body markers. https://github.com/OWASP/www-project-api-security-testing-framework/blob/main/docs/TESTING_GUIDELINES.md
- RESTler authentication docs (primary docs) — token refresh, namespace checker with two users, auth excluded from fuzzing. https://github.com/microsoft/restler-fuzzer/blob/main/docs/user-guide/Authentication.md
- Mayhem for API auth blog (practitioner) — ignore-endpoint, resource hints, rewrite plugins. https://www.mayhem.security/blog/authenticating-with-your-api
- PortSwigger privilege-escalation testing (practitioner) — replay high-priv traffic as low-priv, Autorize statuses. https://portswigger.net/burp/documentation/desktop/testing-workflow/vulnerabilities/access-controls/privilege-escalation
- Trusteed response-diff guide (practitioner) — horizontal/vertical/tenant pairs, diff dimensions, uniform-404 contract. https://trusteed.io/blog/compare-http-responses-between-user-roles-idor-response-diff
- Eresus BOLA/IDOR guide (practitioner) — authorization matrix, delegated access owner+scope+expiry, side-effect oracle. https://www.eresussec.com/en/blog/api-bola-idor-expert-guide
