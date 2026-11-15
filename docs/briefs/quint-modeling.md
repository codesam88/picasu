# Quint modeling

A Quint spec is an executable model, not documentation: the same module text
is run by a simulator (sampled executions), verified by a model checker
(symbolic or explicit-state), and replayed against implementation code as
generated test data. The load-bearing consequence: modeling discipline
(deterministic, guarded, total actions) is what makes the later verification
and model-based-testing modes usable, so it pays for itself at write time.

## Baseline (the obvious first)

Language shape, from the language manual and lessons:

- Modules import modules (`import foo.* from "./foo"`); the entry module is
  chosen per command (`--main` or the file).
- Declarations: `var` (state, only assigned with primed syntax `x' = ...`),
  `val` (constant or stateless value), `def` (operator, may read state and
  assign primed vars = an _action_), `pure def` (state-free), and `run`
  definitions (named test scenarios, e.g. `run myTest = init .then(step)`).
- Nondeterminism is explicit and first-class: `nondet x = S.oneOf()` binds a
  random pick inside an action. Any operator returning a boolean that reads
  state and assigns primed vars composes as an action; actions combine with
  `and`/`or`/`.then` (sequential composition) and `all { ... }`.
- Types: `str`, `int`, `bool`, sets `Set[a]`, maps `str -> a`, records
  `{ field: a, ... }`, tuples; optional fields are modeled as sum types
  (`Some(a) | None`), not a distinct option type.

Tool modes, and what each one claims:

| Command                     | What it does                                                                              | Claim it justifies                                                                                                             |
| --------------------------- | ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `quint test`                | runs the `run` definitions in the module                                                  | scenario-level correctness of the named runs                                                                                   |
| `quint run --init --step`   | simulator: samples random executions, checks invariants at every state                    | "no violation found in N samples" — confidence, not proof; a violation is certain and comes with a counterexample              |
| `quint verify`              | model checking via a backend (`--backend=apalache`, default, or `--backend=tlc`)          | invariants hold for **all** executions within the checker's bound; with inductive invariants, for all executions of any length |
| `quint run --mbt --out-itf` | dumps sampled executions as ITF JSON traces (with `mbt::actionTaken`, `mbt::nondetPicks`) | test _data_ for a driver in the implementation language                                                                        |

Simulator semantics (from "Understanding the Simulator"): OK means no
violation was found in the explored executions — there may still be one.
Simulator counterexamples are not minimal. It supports invariants only, no
temporal properties. Seeds (`--seed`) make runs reproducible; defaults are
`--max-samples=10000`, `--max-steps` bounded per run.

Properties: safety = a boolean expression over state that must hold in every
state (e.g. `val no_negatives = ADDRESSES.forall(a => balances.get(a) >= 0)`);
liveness = temporal formulas, which need fairness assumptions and are much
harder. The docs are blunt: start with safety, it goes a long way.

## The verification ladder (approaches, weakest to strongest)

1. **Simulation** (`quint test`/`quint run`): find the first bugs cheaply.
   The docs' own workflow advice: iterate with the simulator until it runs
   for a long time finding nothing, _then_ reach for a model checker — using
   verify for first bug-hunting means slow feedback cycles.
2. **Bounded model checking** (`quint verify`): proves the invariant for all
   executions up to `--max-steps` (default 10). Always bounded — the result
   is honest only together with the step bound.
3. **Inductive invariants** (`quint verify --inductive-invariant`): a
   stronger invariant proved by base case + inductive step; this gives
   unbounded verification without state exploration. Requirements and
   mechanics:
   - For each state variable, the first occurrence in the inductive
     invariant must be a domain constraint (`x.in(<set>)` or `x == <value>`);
     stronger facts about `x` come only after. The domain predicate is
     conventionally called `TypeOK`.
   - Quint invokes Apalache 2–3 times (base, step, implication) — it
     decomposes into the `--invariant`/`--init`/`--max-steps 0|1` calls for
     you; simulation does not support inductive invariants at all.
   - Finding the strengthening requires expertise (the docs link worked
     examples: TeachingConcurrency, mutex, bakery, reliable broadcast,
     ewd840). Expect iteration: prove something stronger than the goal.
4. **Liveness** (temporal properties): only TLC has real support (Apalache's
   temporal support is partial); needs fairness assumptions. For many
   systems the practical substitute is **witnesses** — "this good state is
   reachable" checked with `--witnesses` (counting) or by negating the
   witness and feeding it to `--invariant` to _print_ an example trace.

## The two verify backends

|                  | Apalache (default)                                                                                                                                                 | TLC (`--backend=tlc`)                                                             |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------- |
| Strategy         | symbolic: translates to SMT constraints, asks Z3 (or CVC5, experimental since 0.58)                                                                                | explicit-state: enumerates the reachability graph                                 |
| Execution length | bounded (`--max-steps`)                                                                                                                                            | any length                                                                        |
| Properties       | invariants; temporal partially                                                                                                                                     | invariants + temporal                                                             |
| Data assumptions | large int ranges fine                                                                                                                                              | nondet must draw from finite sets; infinite sets (e.g. all of `int`) are unusable |
| State space      | handles large/complex state spaces symbolically                                                                                                                    | must be small enough to enumerate; cost grows directly with it                    |
| Maintenance      | active: own org (apalache-mc) since 2024-08, releases ~every 2 weeks (0.63.0 Nov 2026), not organizationally funded — de-facto funded by its small maintainer team | actively maintained (TLA+ foundation)                                             |

Both consume the same spec shape: an `init` action, a `step` action
(typically `one of { ... }` over the system's actions, with each branch
assigning every state variable), and top-level invariant definitions. **The
`step` action is the shared prerequisite of verification and model-based
testing** — write it once, both work.

Apalache integration details that matter operationally: quint talks to it
over an RPC interface, launches an on-demand `apalache-mc server` process
per verify invocation (JVM, ~125 MB), auto-downloads a hardcoded compatible
Apalache version from GitHub releases on first use, and needs a JRE in
`$PATH` (minimum Java 17; 21+ since 0.62). Alternatively, run the server
yourself and point quint at it (`--server-endpoint`). TLC needs no separate
process.

## Model-based testing and the code bridge

MBT direction (model → test data): `quint run spec.qnt --mbt --n-traces=N
--out-itf=traces/` produces N ITF JSON traces; each state carries
`mbt::actionTaken` (which action fired) and `mbt::nondetPicks` (the values
chosen). A test driver replays each trace by dispatching on the action name
and feeding the picks to the implementation's functions, then asserts the
implementation state matches the trace state (and/or returns the same
error/success). The framework compares after **every** step.

For Rust there is a supported library, **quint-connect** (launched 2025-12,
v0.1.x, actively maintained):

- A `Driver` trait: `type State` + `step(&Step)` — pattern-match the action
  (with the `switch!` macro) and call the implementation; `State::from_driver`
  extracts the implementation state, `State::from_spec` the trace state.
- Macros: `#[quint_test(spec = "...", test = "...")]` (replay one named run)
  and `#[quint_run(spec = "...")]` (replay simulated traces). Under the hood
  it invokes the `quint` CLI, writes traces to a tempdir, parses ITF (via the
  `itf` crate), and diffs states on divergence.
- Requires Rust 1.70+, quint on `PATH`.
- The docs position the driver as LLM-maintainable glue — the bulk of MBT
  effort is the concretization (model values → real calls/paths), and they
  argue that's the part AI can maintain.

The opposite direction is **trace validation**: log production/staging
transitions or state snapshots, then ask the model whether the sequence is
possible; "not possible" = the code allows something the model forbids.
Acknowledged as future work in quint-connect; the wiring is meant to be
reusable for it.

What MBT does **not** give: a proof. It gives continuous evidence that code
and spec agree as both evolve — the drift problem (their motivating example:
the 2025 DynamoDB outage — correct design, race introduced by later code
changes).

## Pitfalls and failure modes

- "OK" from the simulator is not verification; and a model checker's OK is
  only as strong as its bound. State both when claiming anything.
- Apalache's "used before assigned" errors: every state variable must be
  assigned on every path of every action; missing assignments surface as
  verification errors, not simulator errors (the simulator may pass a spec
  verify rejects).
- A nondeterministic `step` that mis-weights or omits an action silently
  shrinks what both verify and MBT check — the reachable-state set is exactly
  what `step` defines.
- Simulator counterexamples are not minimal; don't read them as shortest
  reproductions.
- TLC on a model with set-valued state over a large universe explodes; size
  the nondet universes before choosing it.
- Apalache auto-download depends on GitHub reachability and a JRE; in CI,
  prefer a pre-installed Apalache + explicit server endpoint, and pin versions.
- `--max-samples`/`--max-steps` defaults (10k samples) are tuned for
  interactive use; long campaigns need explicit values and recorded seeds.
- Witnesses are reachability checks under sampling — "witnessed in 70% of
  traces" is a coverage signal, not a guarantee that every good state is
  reachable.

## When NOT to use each approach

- Don't start with `quint verify` for debugging the model — the simulator's
  feedback loop is much shorter.
- Don't adopt MBT to find design bugs: it finds implementation drift, after
  the design is settled.
- Don't reach for inductive invariants unless the unbounded claim matters —
  the strengthening effort dominates; for systems where per-run bounds are
  honest (e.g. "within one backstop cycle"), bounded checking is the right
  stop.
- Don't build a refinement chain of specs down to the code "to be close to
  the implementation": the docs call it hard and it still doesn't keep the
  two in sync — that's MBT's job.
- Trace validation is not yet a supported workflow; treat it as research.

## Pointers

- Language + operators: https://quint.sh/docs/lang and the built-ins page
- Property checking, witnesses, inductive invariants:
  https://quint.sh/docs/checking-properties
- Backends: https://quint.sh/docs/model-checkers
- Simulator semantics and workflow advice: https://quint.sh/docs/simulator
- MBT how-to and quint-connect: https://quint.sh/docs/model-based-testing,
  https://github.com/informalsystems/quint-connect
- Apalache management (server, distribution, version pinning):
  https://quint.sh/docs/development-docs/architecture-decision-records/adr008-managing-apalache
- Worked inductive-invariant examples: informalsystems/quint
  `examples/classic/distributed/*`
