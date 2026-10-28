# Auth Policy Engines

Survey of policy languages and verification tooling for API
authorization, scoped to a self-contained Rust binary with no sidecar
services and a coarse role surface (admin / user / share, plus
read-only mode and re-authentication).

## Cedar (recommended if a policy language is adopted)

Cedar is a purpose-built authorization policy language with a Rust
crate (`cedar-policy`, Apache-2.0) that evaluates in-process in
microseconds — no daemon, no HTTP hop. Policies are `permit`/`forbid`
rules over principals, actions, resources, and context, checked
against a schema. The verification story has two halves:

- **Lean proofs** cover the engine itself (evaluator, authorizer,
  validator; 7 properties including forbid-trumps-permit and
  default-deny), not any particular application's policies. Repeating
  that for application properties costs person-weeks per property and
  is out of proportion for a ~74-operation API.
- **The transferable half is differential random testing**: the Lean
  model is executable and runs millions of cases nightly against the
  Rust implementation. A project-local equivalent — a small reference
  authorizer in Rust tested against the Rocket handlers — buys most of
  the assurance without Lean.
- **Cedar Analysis** (CLI, local binary) compiles policies to SMT and
  answers refactor-safety questions: policy equivalence across a
  diff, vacuous/shadowed permits, impossible conditions. That is the
  machine-checkable story for "this change grants nothing new".

Authoring burden is bounded: one schema plus one policy file per
role, days of work at this scale. No off-the-shelf generator emits
API tests from Cedar policies; the harness that enumerates
operation × role → expected decision is project code, but it is
small because the crate is in-process and fast.

## OPA / Rego (best test ergonomics, fails deployment)

Rego has the most mature policy-test runner (`opa test` with
data-driven cases, mocking, coverage, CI-ready JSON). But OPA is a
Go daemon: the canonical REST pattern POSTs every decision to a
sidecar over HTTP. Embedding in a single-binary Rust product means a
process dependency or immature FFI. Adopt only if policy evaluation
ever externalizes to a sidecar, which is not planned.

## OpenFGA (avoid at this scope)

Relationship-graph (Zanzibar-style) authorization over a tuple
store behind a gRPC/HTTP server. RBAC can be encoded in it, but the
modeling direction is relationships, and three coarse roles plus a
mode flag gain no expressive power from graph traversal. Requires
running a server; no embeddable Rust engine. Revisit only if
per-resource sharing graphs appear.

## SAPL (avoid)

JVM policy engine (streaming obligations, behavior-driven test
DSL). No Rust crate; embedding needs JVM/GraalVM in a Rust
single-binary product. Heavy for the need.

## NYX scanner (complement, not model)

`nyx scan` is static analysis (AST/CFG/taint) that flags missing
ownership/membership checks in handlers, including cross-function
helper summaries. It is an assurance scanner, not a declarative
model — it cannot drive implementation from a spec. Useful in CI
alongside a policy model, not instead of one.

## Bottom line

If a policy language enters the tree, it is Cedar-as-library with
Cedar Analysis in CI and a small Rust decision harness; Lean proofs
of application properties stay out. Everything else either needs a
sidecar or models a problem (relationship graphs) this API does not
have.

## Pointers

- Cedar verification-guided development (Lean model + DRT, proof
  costs, bug yields): <https://arxiv.org/html/2407.01688v1>
- AWS on Lean verification practice:
  <https://aws.amazon.com/blogs/opensource/lean-into-verified-software-development/>
- Cedar Analysis (SMT policy equivalence, shadowing, vacuity):
  <https://aws.amazon.com/blogs/opensource/introducing-cedar-analysis-open-source-tools-for-verifying-authorization-policies/>
- Cedar Rust crate: <https://docs.rs/cedar-policy/latest/cedar_policy/>
- Cedar OpenAPI-to-schema middleware (API → policy direction):
  <https://docs.cedarpolicy.com/auth/integration-express.html>
- OPA policy testing: <https://openpolicyagent.org/docs/policy-testing/>
- OPA HTTP API authorization (sidecar pattern):
  <https://openpolicyagent.org/docs/http-api-authorization>
- OpenFGA roles modeling: <https://openfga.dev/docs/modeling/roles-and-permissions>
- NYX auth analysis: <https://nyxsec.dev/docs/nyx/auth>
