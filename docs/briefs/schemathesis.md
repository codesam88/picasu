# Schemathesis

Property-based API tester: it reads an OpenAPI (or GraphQL) schema,
derives input generators from it, sends requests, and judges responses.
Out of the box it finds crashes and schema violations; every
project-specific invariant needs a custom check. That split — free
oracle vs handwritten oracle — is the load-bearing idea for scoping
any Schemathesis work.

## Baseline (the obvious third)

Hypothesis strategies drive values from the schema through four phases
(examples, coverage, fuzzing, stateful); failures shrink to minimal
reproducers with `curl` output. Stateful testing chains producers to
consumers via inferred, `Location`-learned, or manual OpenAPI links
(applied probabilistically — random IDs next to real ones are by
design) against disposable environments only. Extension is hooks
(`filter_`/`map_`/`flatmap_` over request parts, `before_call` as the
universal hatch) plus custom checks (function per-response, class with
`after_run` for run-wide invariants), loaded as a module or
`conftest.py` with a TOML config. Auth layers run from static headers
through OpenAPI-aware schemes to declarative token endpoints (body,
header, or cookie extraction with refresh) and Python auth classes;
explicit configuration disables auth classes, and the runner
deliberately drops credentials on some requests to verify rejection.

## The adaptive loop (what a single run hides)

The distinguishing behavior is in-run learning, and misreading it
produces the classic pilot confusions:

- **Error-feedback constraint learning.** Rejected positive-mode 4xx
  responses with recognized framework envelopes become validation
  rules applied to the operation's schema from the next checkpoint —
  the tool literally rewrites its generators mid-run. The trade-off is
  sharp: learned constraints push traffic toward business logic but
  hide bugs behind validation, which only negative mode still probes.
  A run that "goes quiet" may be learning, not passing.
- **Resource pool.** Successful responses populate per-resource pools
  that all phases draw from (producers ordered before consumers), so
  success paths get exercised without any link. The stateful phase
  reads the same pool for parameters its links don't fill.
- **Per-run cache.** Rejected requests, inferred auth requirements,
  and 405s persist across runs and replay at the next start — runs
  are not independent, and "from scratch" needs explicit cache
  clearing.
- **Auth inference and budget shifting.** 401/403 on public operations
  infers requirements and retries with configured credentials;
  signal-less operations get deprioritized so budget moves. Our pilot
  saw all three fire.

## Stateful testing, properly understood

Schemathesis links are one instance of Hypothesis's rule-based
stateful testing: named bundles carry values between rules, programs
shrink to minimal demonstrators, preconditions (not assumptions)
select applicable rules, invariants run after every step. Knowing the
ancestor explains the descendant's edges — e.g. why link-less
parameters still sometimes hit real IDs (pool, not links), and why
chains need disposable state. The deeper ancestor is RESTler (ICSE'19,
the first stateful REST fuzzer): static producer-consumer inference
from the spec plus dynamic feedback learning refused sequences, with
feedback alone cutting 40x noise dramatically. RESTler's lesson that
transfers: _both_ inference and feedback are needed; either alone
underperforms. Schemathesis's Python-native strategies and hooks are
the reason to prefer it; RESTler's grammar-based config and
security-bug track record on cloud services are the reason it stays
the fallback when auth customization proves insufficient.

## Extension sharp edges

- **Phase-caveat matrix.** Component hooks (`filter_*`/`map_*`) never
  run in coverage and never alter schema-authored example values;
  only `before_call` and `[parameters]` overrides apply everywhere.
  A hook that "does nothing" is usually firing in a phase that
  ignores it — our pilot's share-header wiring had to be verified
  per phase for exactly this reason.
- **Check accumulation must be idempotent.** Shrinking and replay
  re-invoke per-response hooks; prefer sets over counters. Class
  checks see partial traffic under parallel execution (`after_run`
  - xdist is broken by design).
- **Check kwargs come from config.** `__init__` parameters matching
  `[checks.<Name>]` keys are forwarded automatically — per-project
  tuning without code changes; checks also disable individually.
- **Negative traffic is signal, not failure.** Deliberately-wrong
  inputs are the default in non-example phases; 4xx noise means the
  fuzzer works. Triage is bug / harness artifact / spec imprecision —
  and undocumented framework statuses (our 422s) drown real findings
  until the contract is reconciled.

## Limits that scope the work

The built-in oracle ends at 5xx and schema conformance. Absence of
effect, cross-operation data isolation, token lifecycles, and races
are custom checks or handwritten tests — and the token choreography
behind them is not inferable from schema alone, so the harness
staging identities and fixtures is the irreducible handwritten part.
Cookie-session logins defeat auto-detection entirely. Stateful runs
need budgets an order of magnitude past what reaches a handful of
operations, and accepted findings need a baseline file before any
gate can fail a build.

## Pointers

- Schemathesis _Adaptive testing_ explanation — read for the learning
  loop, resource pool, cache, and the narrowing trade-off:
  <https://schemathesis.readthedocs.io/en/stable/explanations/adaptive-testing/>
- Schemathesis _Stateful testing_ explanation — read for link sources,
  extensions (regex, nested bodies), and probabilistic application:
  <https://schemathesis.readthedocs.io/en/stable/explanations/stateful/>
- Hypothesis _Stateful tests_ — read for the bundles model behind the
  resource pool, plus preconditions/invariants:
  <https://hypothesis.readthedocs.io/en/latest/stateful.html>
- Atlidakis et al., _RESTler: Stateful REST API Fuzzing_ (ICSE'19) —
  read for dependency inference + dynamic feedback and when RESTler
  instead: <https://www.microsoft.com/en-us/research/publication/restler-stateful-rest-api-fuzzing/>
- Schemathesis auth + extending guides — read when wiring credentials
  or custom checks (precedence, refresh, scoping):
  <https://schemathesis.readthedocs.io/en/stable/guides/auth/>
  <https://schemathesis.readthedocs.io/en/stable/guides/extending/>
