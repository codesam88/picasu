# Schemathesis Modeling

What Schemathesis can and cannot model, and which direction
derivation flows between the auth model and the fuzz harness. Read
alongside the companion brief on the runner itself.

## The harness is a consumer, not a model

Schemathesis reads the OpenAPI document plus its own TOML config and
Python hooks; nothing flows back out as a declarative artifact. The
"model" at runtime is an in-memory Hypothesis state machine
(transitions learned from schema analysis, `Location` headers, and
explicit OpenAPI Links) plus adaptive learning (4xx constraint
rewriting, resource pools, auth inference). None of it serializes:
no derived config, no exported state machine, no generated links.
Making the harness the source of truth would lock heuristic,
run-dependent behavior (sign-up path guessing, probabilistic link
application, per-run caches) into the contract.

## What each extension point expresses

- **OpenAPI Links** chain producer → consumer with exact or
  regex-extracted values, declared under the status code actually
  returned. The fuzzer does not create data, only chains real
  requests against disposable state.
- **Custom checks** are per-response functions or classes with
  `after_run` for run-wide assertions. Absence-of-effect and
  cross-operation isolation are hand-coded state accumulation, not
  built-ins — except ST402 (object-level authorization), which replays
  each peer's object reads as every other peer and is driven by an
  external Web Fuzzing Commons auth document (peers + login), not by
  the spec alone. That WFC document is the pattern to copy: the
  declarative auth matrix lives outside the harness, the harness
  executes it.
- **Auth** runs from static headers through OpenAPI-aware schemes to
  declarative token endpoints (body/header/cookie extraction with
  refresh) and programmatic auth classes, plus heuristic auto
  sign-up+login (4.30.0+) that only fires on conventional register
  and login shapes and disables itself when credentials are
  supplied. Cookie sessions and cross-token-type choreography
  (demoted tokens, wrong-typ rejection) stay in hand-written hooks.
- **Generation controls** (`max-examples`, phases, stateful
  step count, deterministic mode, serializers) are test-volume knobs
  — harness parameters, never contract.

## Review and docs artifacts (without being the spec)

Schema coverage reports (TraceCov dimensions: operations,
parameters, status codes, auth), JUnit XML, HAR/VCR cassettes of
observed traffic, crash files with replay-until-clean semantics,
the accepted-findings baseline, and check-semantics references all
document what was exercised and found. None defines behavior; the
baseline in particular is a triage ledger that must shrink as bugs
fix, not a requirement set.

## Derivation direction

Declarative auth metadata in the OpenAPI document (per-operation
security, schemes, links) plus a peers/login matrix in the WFC
shape → generated or hand-synced harness inputs (auth injection,
seeded fixtures, probe parameters). Findings flow back as spec
corrections, the same loop as any contract gate. The first
derivation worth building is auth-matrix → hook inputs (killing the
hand-maintained guard-operation lists), then links for the token
renewal chains.

## Pointers

- Stateful testing (links, inference, probabilistic application):
  <https://schemathesis.readthedocs.io/en/stable/explanations/stateful/>
- Custom stateful behavior (setup/teardown, per-call hooks):
  <https://schemathesis.readthedocs.io/en/stable/guides/stateful-testing/>
- Extending (function vs class checks, `after_run`):
  <https://schemathesis.readthedocs.io/en/stable/guides/extending/>
- Object-level authorization / ST402 (WFC-driven):
  <https://schemathesis.readthedocs.io/en/stable/guides/object-level-authorization/>
- Authentication (schemes, token endpoints, auto sign-up):
  <https://schemathesis.readthedocs.io/en/stable/guides/auth/>
- Adaptive testing (constraint learning, pools, caches):
  <https://schemathesis.readthedocs.io/en/stable/explanations/adaptive-testing/>
- Data generation (phases, shrinking, determinism):
  <https://schemathesis.readthedocs.io/en/stable/explanations/data-generation/>
- Configuration reference: <https://schemathesis.readthedocs.io/en/stable/reference/configuration/>
- Baseline, crash reproduction, coverage, CI integration:
  <https://schemathesis.readthedocs.io/en/stable/guides/baseline/>
  <https://schemathesis.readthedocs.io/en/stable/guides/crash-reproduction/>
  <https://schemathesis.readthedocs.io/en/stable/guides/coverage>
  <https://schemathesis.readthedocs.io/en/stable/guides/cicd/>
