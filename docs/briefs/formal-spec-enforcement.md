# Brief: Formal specs that stay enforced on code

Researched: 2026-11-09 | Versions: Sail (rems-project, rolling), Arm ASL1 ref DDI0626 (Mar 2026), Isla/isla-testgen (rolling), RISC-V Sail model (official) | Overall confidence: high — all central claims rest on primary maintainer sources below

Scope: the pattern below is domain-independent — one machine-readable source, generated artifacts, an executed spec, measured coverage, deliberate redundancy. Instruction-set architectures (RISC-V, Arm, CHERI/Morello) are the worked example throughout: they exhibit every failure mode at scale, so each rule names its ISA instance as evidence, not as its subject. Transfer to any subsystem the same way: replace “instruction” with the domain's unit of behavior and “conformance suite” with the domain's acceptance checks.

## 1. Baseline

A formal specification is a precise, machine-readable description of
allowed behavior. The mature pattern has four parts: (a) a
human-readable reference that embeds the formal text beside the
prose; (b) an executable spec usable as a golden-reference simulator
and test oracle; (c) machine-checked derivations — generated
conformance tests, symbolic evaluation, theorem-prover definitions
where justified; (d) a feedback loop where every disagreement
(simulator trace, failed proof, coverage hole) becomes a spec fix or
a regression test, never a shrugged-off log line. The spec language
is deliberately weak and inexpressive: imperative, first-order,
engineer-familiar pseudocode plus dependent types only where the
domain needs them (e.g. bitvector lengths) and explicit
nondeterminism. Anything stronger invites modeling cleverness that
no implementer or auditor can follow.

## 2. What research adds

- **Generate, don't transcribe.** The dominant failure mode is N
  artifacts maintained in N repositories with nothing generated from
  anything else: each extension is then written N times and checked
  by review. Rule: capture the checkable content once in a
  machine-readable form (JSON/YAML for tables, encodings, and
  field metadata; a small executable spec language for behavior)
  and generate the documents, simulators, tests, and
  prover definitions from it (ISA instance: instruction
  encodings and register descriptions generating the manual,
  the simulator, and the testsuite from one XML source).
  Then refactor the _existing_
  artifacts around the generated code so new entries are
  generatable too. Rationale: transcription diverges silently;
  generation diverges loudly or not at all. Sources: Reid
  riscv-spec-issues (four disconnected RISC-V artifacts),
  ARM-v8a-xml-release (XML as the single source for manual,
  simulator, tests).
- **The spec must be executable, or it is decoration.** An
  executable spec doubles as the golden reference: run the same
  acceptance checks against spec and implementation and diff
  (ISA instance: the architecture conformance suite run
  against both the ASL spec and each processor). Rule: gate on executing the spec in
  CI (typecheck plus test runs), and keep a simulator for
  exploration strictly advisory — exploration finds traces,
  gates check them. Rationale: an unexecuted spec rots exactly
  like an untested code path. Sources: Sail README (emulator
  generation), Reid ARM-v8a-xml-release (conformance suite on
  the spec).
- **Interleave formal text with prose, in both directions.**
  Formal-only specs go unread; prose-only specs go untested.
  Put each spec fragment on the same page as the natural-language
  description of the same feature, generate
  doc snippets from the spec,
  and require comments in the spec where the prose lives
  elsewhere (ISA instance: CHERI ISAv9 style interleaving;
  Sail LaTeX/AsciiDoc backends). Rule: no checked-in formal text without an adjacent
  human sentence saying what it means, and no behavioral prose
  claim without a check behind it. Rationale: the two
  representations catch different mistakes. Sources: Reid
  riscv-spec-issues (Sail-in-manual, CHERI example),
  sail-riscv NAS 2025 slides (embedded Sail fragments went
  obsolete — interleaving without a generation/check gate
  still drifts).
- **Test the spec with its own tools: coverage and generated
  tests.** Measure which spec branches the test suite exercises
  and generate tests from the spec via symbolic execution or
  randomized exploration (ISA instance: `sailcov` coverage;
  Isla + `isla-testgen` producing full-path tests from the
  model). Rule: track spec
  coverage as a metric and close holes with generated runs,
  not hand-written examples. Rationale: hand-written tests
  cover what the author already believes; generated tests
  cover what the spec actually says. Sources: isla-testgen
  README (Morello/CHERIoT use), Isla paper §evaluation
  (3798 litmus tests vs Herd).
- **Reintroduce redundancy deliberately — automation removes
  expert eyeballs.** When transcription is automated, fewer
  experts read the spec from different angles, and some
  mistakes stop being caught. Rule: keep at least two
  _independent_ derivations of each critical property that
  must agree (e.g. model runs vs. stateful implementation
  tests; generated negatives vs. handwritten flows), plus
  differential checks between old and new representations
  during migrations. Rationale: agreement of independent
  derivations is the cheapest substitute for the lost
  eyeballs. Sources: Reid mrs-at-scale (the “downside”
  section), Sewell full-scale talk (proof + testing + test
  generation used together on Morello).
- **Specify the envelope, not an instance.** Highly
  configurable systems need a machine-readable description of
  which configuration is intended and what the full envelope
  of allowed behaviors is; otherwise every out-of-range
  build drowns in waivers (ISA instance: untestable
  configuration envelope forcing per-build waiver lists).
  Rule: configuration and bounds
  are part of the spec (small explicit bounds, recorded
  where each bound bites), and anything outside the
  envelope is a stated non-goal, not a silent gap.
  Rationale: waivers are drift with paperwork. Sources:
  Reid riscv-spec-issues (configurability complaint),
  Sail litmus work (envelope of allowed concurrency
  behavior as the spec output).
- **Distill, don't invent: one authoritative spec from many
  sources of truth.** Real systems already have many partial
  sources (products, simulators, suites, docs). Rule: write
  the spec by reading it off the strongest sources,
  validate the result against _all_ of them, and record
  each disagreement as a contract or test (ISA instance:
  distilling products, simulators, suites, and docs into
  one authoritative model). Never design the
  golden model up front and let it lag the implementation.
  Rationale: a spec taken on faith goes stale on day one.
  Sources: Reid mrs-at-scale (distillation rule).
- **Keep the language weak and the audience in mind.**
  Prefer the most inexpressive language that covers the
  domain; write it to look like the pseudocode the
  domain already reads; use defaults and inference to
  cut type-system clutter; never require PL background
  to read the common case. Rule: if a construct needs a
  comment explaining the _language_ rather than the
  _domain_, it does not belong in the common path
  (ISA instance: ASL/Sail kept readable to hardware,
  compiler, and OS engineers with no PL background).
  Rationale: the readers are domain engineers, not PL
  researchers; an unread spec is an unenforced one.
  Sources: Reid riscv-spec-issues (Sail accessibility,
  ASL-1.0 comparison), ASL readers' guide (audience:
  programmers, hardware and verification engineers).
- **When NOT to do all of this.** Full multi-backend
  formalization pays at ISA scale (thousands of engineers,
  decades of history, hardware respins). For a small team:
  one executable spec, one gate, generated tests where
  the behavior space is large — and stop. Rule: each
  backend (prover definitions, symbolic execution,
  hardware generation) must justify itself with a real
  bug class or a real consumer (ISA instance: full
  multi-backend formalization pays for thousand-engineer
  chip programs with respin costs; not for small teams);
  otherwise it is
  maintenance without enforcement. Sources: Reid
  mrs-at-scale (scale framing), Sail discussion #671
  (isla-testgen is research-grade — pin versions,
  expect bitrot).

| Property kind              | Cheapest enforcing mechanism                         | Escalate to when it fails to catch real bugs      |
| -------------------------- | ---------------------------------------------------- | ------------------------------------------------- |
| Single-call-site rule      | Constructor-checked types (omission = compile error) | A project-specific lint over the crate            |
| Pure decision logic        | Generated property tests with shrinking              | Exhaustive model runs on finite bounds            |
| Cross-trigger convergence  | Scripted interleaving runs asserting settled state   | Random-trace simulator with classified transients |
| Cross-artifact consistency | Regenerate-and-diff gate in CI                       | Independent derivation that must agree            |
| Human understanding        | Formal fragment beside its prose sentence            | Review checklist for the uncovered shape          |

## 3. Worker checklist

Do:

- Keep one machine-readable source per fact; generate
  docs, tests, and checks from it; diff generated output
  against committed artifacts in CI.
- Execute the spec in CI (typecheck + scripted runs);
  keep random exploration advisory with trace triage.
- Measure spec coverage; close holes with generated,
  not hand-written, cases.
- Interleave each formal fragment with its human
  sentence; comment the _domain_, never the _language_.
- For each critical property, keep two independent
  derivations that must agree.
  Don't:
- Maintain the same rule in two places by hand.
- Check in formal text no gate executes.
- Let bounds, configurations, or waivers live
  outside the spec.
- Add a prover/symbolic backend without a named bug
  class or consumer.
  Validate by: committed-artifact diff gates green,
  coverage non-decreasing, every simulator/prover
  finding traceable to a spec fix or a regression
  test, zero un-triaged traces.

## 4. Sources

Worked-example evidence for the pattern above, ranked: exemplar code and primary docs first, then secondary. One line each on why it matters.

- Reid, riscv-spec-issues (2024, primary — maintainer
  diagnosis): disconnected artifacts, generate-from-one-source,
  interleave formal+prose, weak accessible language. Why it
  matters: the failure catalog this brief inverts.
- Reid, mrs-at-scale (2022, primary): weak language,
  distill sources of truth, spec-as-documentation, deliberate
  redundancy, scale framing. Why: the principles behind the rules.
- rems-project/sail README (primary, rolling): backends
  (C/OCaml, coverage, Isla, Coq/Isabelle/HOL4, doc snippets).
  Why: the exemplar enforcement toolchain.
- Arm ASL1 reference DDI0626 + readers' guide (primary,
  2025-09/2026-03): engineer-readable executable spec,
  explicit nondeterminism, EAC-quality sequential core.
  Why: proof the weak-language pattern ships at scale.
- Reid, ARM-v8a-xml-release + using-asli (primary):
  XML single source, conformance suite executed against
  the spec, transpiler-per-use. Why: generation pattern
  with receipts.
- riscv-nasummit2025 Sail slides (primary): embedded
  Sail fragments went obsolete. Why: interleaving
  without a gate still drifts — the negative result.
- Isla paper, Springer FMSD 2023 (primary): symbolic
  execution of full-scale Sail, 3798 litmus tests vs
  Herd. Why: generated-test evidence at scale.
- isla-testgen README (exemplar code): model-based test
  generation for Morello/CHERIot, coverage workflow.
  Why: the concretes for “close holes by generation”.
- Sewell, full-scale formal architecture talk (secondary):
  proof + testing + generation combined on Morello.
  Why: independent-derivations precedent.
