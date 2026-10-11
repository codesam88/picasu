# Schemathesis

Property-based API tester: it reads an OpenAPI (or GraphQL) schema,
derives input generators from it, sends requests, and judges responses.
Out of the box it finds crashes and schema violations; every
project-specific invariant needs a custom check. That split — free
oracle vs handwritten oracle — is the load-bearing idea for scoping
any Schemathesis work.

## Mental model

Three layers, each doing one job:

1. **Hypothesis** — primitive strategies, shrinking a failure to its
   minimal reproducer, and an example database across runs.
2. **Schema-strategy engine** — translates schema fragments into
   Hypothesis strategies (JSON Schema engine for OpenAPI). This is the
   per-operation value source every phase draws from.
3. **Schemathesis pipeline** — four test phases, HTTP transport,
   response checks, and a feedback loop that learns from server
   responses (real IDs observed get reused; effort shifts away from
   operations producing no signal).

## Phases

Each phase adds its own cases per operation; an operation's total
traffic is the sum across phases.

- **Examples** — sends the schema's own `example`/`examples` values.
  Positive-only; the only phase that trusts the schema author.
- **Coverage** — boundary values per schema constraint (length 2..10
  draws 1, 2, 3, 9, 10, 11). Deterministic count from the constraints.
- **Fuzzing** — random valid and invalid data from the constraints.
  This is where deliberately-wrong inputs come from.
- **Stateful** — chains operations with real response data (see below).
  Runs only when at least one connection between operations is known.

Generation modes (positive / negative / all) control whether cases
should be accepted or rejected by the API; non-example phases default
to both, because rejected-input handling is where most server bugs
live. Expect 4xx noise in every run — it is the tool working, not a
failure. Case volume per operation is configurable; shrinking means a
single finding costs extra requests while it minimizes.

## Stateful testing: links

Stateful testing chains producers to consumers (create → read →
update → delete) using real IDs. Connections come from three sources
that combine additively:

1. **Automatic schema analysis** — infers resource production and
   consumption from response shapes and parameter names (path params,
   nested resources, pagination envelopes, composed schemas).
2. **Location-header learning** — observes `Location` headers during
   earlier phases and links them into operations. Only happens in CLI
   runs where the earlier phases actually execute.
3. **Manual OpenAPI links** — explicit `links` declarations for
   relationships analysis misses or non-path dependencies. Extended
   beyond the standard: regex extraction from headers/parameters and
   nested expressions in request bodies.

Two behaviors to internalize: links apply _probabilistically_ — random
IDs appear next to real ones by design, so a 404 inside a chain does
not mean a link is broken; and stateful runs _mutate data_, so they
only ever target disposable environments.

## Extension model: hooks and checks

Customization has two channels, loaded as a Python module (CLI) or
`conftest.py` (pytest), plus a TOML config file:

- **Hooks** reshape generation: `filter_<part>` drops cases,
  `map_<part>` rewrites values, `flatmap_<part>` re-derives a value
  from an already-drawn one via a dependent strategy. Parts are query,
  headers, path parameters, cookies, body, or the whole case; hooks can
  be scoped to operations by path, method, name, tag, or ID. Two
  caveats that bite: the coverage phase bypasses component hooks
  entirely, and the examples phase never lets hooks alter
  schema-authored values. `before_call` is the escape hatch that runs
  on every request in every phase.
- **Custom checks** judge responses. Function checks assert one
  per-response invariant and raise on violation. Class checks
  accumulate across responses for run-wide invariants, with an
  `after_run` hook — but accumulation must be idempotent (shrinking
  and replay re-invoke per-response hooks) and `after_run` sees only
  partial traffic under parallel execution.
- **Config file** holds what shouldn't be code: parameter overrides
  (pin a value in _every_ phase, hooks can't), per-check tuning and
  enabling, per-operation header/auth scoping.

## Authentication model

Credentials come in layers; knowing which layer a target needs avoids
the common "everything 401s" run:

- **Auto sign-up/login** — detects a register+login pair returning a
  bearer token or API key and exercises it unconfigured. Narrow: only
  bearer/key schemes, and **cookie-session logins are not detected**.
  Anything else needs explicit configuration; unauthenticated
  operations are listed under run warnings rather than failing.
- **Static credentials** — fixed headers or basic auth, globally or
  per operation. Simple, no refresh.
- **OpenAPI-aware schemes** — values bound to the schema's named
  `securitySchemes` (location and name come from the spec, only the
  secret is supplied).
- **Declarative dynamic auth** — a token-fetch endpoint plus a
  selector (JSON Pointer into the body, a header name, or a cookie
  name); the token is cached and periodically re-fetched. Covers
  form-encoded logins and cookie sessions declaratively.
- **Python auth classes** — `get` fetches, `set` attaches; supports
  refresh schedules, per-scope token caching, and selective
  application to operation subsets. The escape hatch when nothing
  declarative fits.

Precedence matters: any explicit configuration disables auth classes,
and CLI flags override the config file. One deliberate behavior to
know: the runner _removes or alters_ credentials on some requests to
verify the API rejects unauthenticated calls — a 401 probe inside a
run is the tool testing your auth, not losing it.

## Oracle limits

Built-in checks end at server errors and schema conformance. Absence
of effect ("refused write changed nothing"), cross-operation data
isolation ("share-A rows never reference album B"), token-lifecycle
rules, and anything concurrent are custom checks or handwritten tests
— the token choreography behind them is not inferable from schema
alone, so the harness that stages identities and fixtures is the
irreducible handwritten part of any engagement.

## Sources

Schemathesis readthedocs (concepts: data generation, stateful
testing; how-tos: authentication, extending, stateful runs), surveyed
for the api-assurance plan. Re-verify against current docs when
starting new work; record the outcome in the using plan.
