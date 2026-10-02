---
status: in-progress
type: feature
priority: high
area: backend
---

## Notes

The concrete assertions this repository wants on every `#[utoipa::path]`
annotation, replacing the source/spec rule set that was deleted with the
`openapi-sanity` crate. Drafted 2026-09-28 after `rocket_extras` (the annotation
no longer restates its route), `#[utoipauto]` (no file list) and
`--check-openapi` (route-set parity proven at runtime) landed.

### The filter

A check belongs here only if **both** hold:

1. It reads a fact that exists only in source — not in the document, not in
   Rocket's mount table.
2. Nothing else already enforces it: not rustc, not utoipa's derivation, not
   `openapi-json-match`, not `--check-openapi`, not the committed-artifact diff.

### Rule index

Every rule in this plan has an id, and an id is how it is referred to — in review,
in a commit message, in a `just` recipe comment. **An id is stable:** sharpening a
rule's wording does not change its id, a retired id is never reused, and a new rule
takes the next free number in its letter's series. "Where it lives" says who
enforces the rule today, which is the question a reader of this table usually has.

| id  | rule                                                                                                                           | status         | where it lives                                                                        |
| --- | ------------------------------------------------------------------------------------------------------------------------------ | -------------- | ------------------------------------------------------------------------------------- |
| A1  | the annotation restates neither `path` nor a bare verb                                                                         | enforced       | `utils/openapi-sanity`                                                                |
| A2  | the annotation declares at least one response                                                                                  | enforced       | `utils/openapi-sanity`                                                                |
| A3  | the annotation declares exactly one `tag` from the vocabulary                                                                  | enforced       | `utils/openapi-sanity`                                                                |
| A4  | the handler carries a doc comment                                                                                              | enforced       | `utils/openapi-sanity`                                                                |
| A5  | the doc comment's first paragraph is one line                                                                                  | enforced       | `utils/openapi-sanity`                                                                |
| A6  | the annotation sets no `operation_id`                                                                                          | enforced       | `utils/openapi-sanity`                                                                |
| A7  | the annotation sets neither `summary` nor `description`                                                                        | enforced       | `utils/openapi-sanity`                                                                |
| C1  | a `GuardResult<…>` argument has its rejection propagated                                                                       | enforced       | `utils/openapi-sanity`                                                                |
| C1b | a plain `Guard…` argument needs nothing in the body and is never reported                                                      | enforced       | `utils/openapi-sanity`                                                                |
| B1  | every declared `params(...)` name corresponds to a route segment or query binding                                              | enforced       | `utils/openapi-sanity` — inline tuple form only, see section B                        |
| B2  | a declared parameter's `required` equals `!argument_is_option`                                                                 | enforced       | `utils/openapi-sanity` — utoipa has no `required` key, see section B                  |
| B3  | a declared `request_body` schema equals the type the route's `data = "…"` binds                                                | enforced       | `utils/openapi-sanity` — `Value` declares no constraint, see section B                |
| B4  | a `Form<…>` binding declares `multipart/form-data`                                                                             | enforced       | `utils/openapi-sanity` — found two form bodies declared as JSON                       |
| C2  | every `pub fn generate_*_routes()` is mounted in `router/builder.rs`                                                           | specified      | section C — not built                                                                 |
| M1  | every `POST`/`PUT` route carries `GuardReadOnlyMode`                                                                           | specified      | section M — not built                                                                 |
| M2  | a route carrying the mode guard documents a `405`                                                                              | specified      | section M — not built                                                                 |
| M3  | the mode guard never appears in `security(...)`                                                                                | specified      | section M — not built                                                                 |
| M4  | the mode guard's rejection is propagated                                                                                       | specified      | section M — not built                                                                 |
| D7  | every `POST`/`PUT` route carries at least one credential guard, or is listed as deliberately public                            | new — blocked  | open decision, on Q1                                                                  |
| D8  | the credential set an operation declares in `security(...)` equals the credential set its route's guards provide               | new — blocked  | open decision, on Q1 and the scheme open decision below                               |
| A8  | every guard binding is named after its guard class, `_`-prefixed when the value is discarded                                   | new            | section C — not built, calibrated below                                               |
| C3  | a route carrying a guard documents the status that guard rejects with — `401` for a credential guard, `405` for the mode guard | new            | section C — not built                                                                 |
| R1  | a `POST` route lives under `/post/`, or is listed as deliberately placed elsewhere                                             | new            | not built                                                                             |
| S1  | a handler defined in `router/get/get_page.rs` is tagged `pages`, and no handler defined elsewhere is                           | new — measured | not built, calibration below                                                          |
| P1  | a handler's documented success status matches what it returns (204 for no body)                                                | spike          | spike first: measure the return-type analysis cost and report before writing the rule |
| V1  | a credential comparison is constant-time                                                                                       | review-only    | no checker can see it — "Review rules" below                                          |
| Q1  | are these five routes deliberately public?                                                                                     | open question  | the backend, not this plan — the answer makes D7 writable                             |
| Q2  | should `POST /get/prefetch` publish its filter grammar as a schema, or describe it in prose?                                   | open question  | `.plan/bug-prefetch-request-body.md` — a public-API decision, not a tool rule         |

### What the deleted checker covered, and what covers it now

| Deleted rule                                                                           | Now enforced by                                                                            |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| registered in `routes![]` without an annotation                                        | rustc (the derive references `__path_<fn>`) plus `--check-openapi` (mounted, undocumented) |
| source↔document, both directions                                                       | `openapi-json-match`, `openapi-routes-match`                                               |
| route verb and path vs annotation                                                      | `rocket_extras`: the route's spelling wins, so there is nothing to compare                 |
| route segments/query vs declared parameter names                                       | `rocket_extras` derives them; the residue is B1/B2                                         |
| tag policy, `operationId` uniqueness, `$ref` and orphan schemas, missing `operationId` | document-shaped; nothing enforces them, which is a deliberate gap recorded below           |

### Guard tokens — the authentication surface is currently undocumented

`backend/openapi.json` declares **no `securitySchemes` and no per-operation
`security`**: 61 operations, zero authentication requirements. A consumer of the
spec learns that an operation may answer `401` and nothing else. The one artifact
that used to record it — `AUTH_POLICY`, 61 entries keyed by `operationId` — was
deleted with the crate, and its guard rule caught a real bug (`84f29aa5`, the
discarded `GuardResult`). So the checker's guard rules are not a regression
against a covered area; they are the replacement for the only record we had.

Seven guard classes exist today, and they are three different things:

- **Authentication** (they reject a caller who cannot prove who they are, and
  belong in the document): `GuardAuth`, `GuardTimestamp`, `GuardHash`,
  `GuardHashOriginal`, `GuardShare`, `GuardUpload`.
- **Mode restriction** (they constrain what this build may do, and are not
  authentication): `GuardReadOnlyMode`.
- **Explicit re-authentication** (decided 2026-09-28, not yet implemented): a
  valid token is not enough, the caller must present the account password again.
  It becomes its own guard rather than a field in a request body, because two
  operations need it and the requirement belongs to the operation rather than to
  one handler's payload: disabling read-only mode
  (`.plan/bug-readonly-lockout.md`) and changing the user password. Because the
  requirement is _in addition to_ the token, these operations declare two
  security requirements — the token's and the re-authentication scheme's, which
  D5 requires them to name and D6 requires never to appear without the token.

The design, in two parts:

- **Standard, consumer-visible.** Register the security schemes in the document
  and declare `security(...)` on every operation whose route carries an
  authentication guard. `securitySchemes` does not exist in the document yet and
  must be added to `ApiDoc`'s `components(...)`.
- **The class is read from the route, not from the document.** An operation
  cannot say _which kind_ of authentication it needs in standard OpenAPI — a
  `security` list names schemes, and the scheme is what carries the kind. So the
  kind lives in the guard on the route, and D8 is the rule that makes the two
  sides agree: the credential set an operation declares equals the credential set
  its route's guards provide. That is checkable; "which kind" spelled out in an
  extension is not.

#### Decision (2026-10-02): `security(...)` only, no `picasu`-prefixed extension

The open question was whether to add a repo-specific extension in the shape
already used for features —
`extensions(("x-picasu-constraints" = json!(["auth", "read_only_mode"])))` — to
say _which kind_ of constraint a route carries. **The answer is no.** The
document carries standard `security(...)` and standard `securitySchemes`, and
nothing else. Three reasons, in the order they decided it:

1. **The extension could not say what it claimed to say.** The list it would
   carry is constraints, not only authentication, so `x-picasu-auth` would be a
   slight lie on the mode and re-authentication routes, and
   `x-picasu-constraints` would be a second, parallel vocabulary that a consumer
   has to learn before it can read the spec. A reader who has the standard and
   not the extension learns less about authentication, not more.
2. **The kind is already recoverable, and D8 is what makes that true.** A scheme
   name is a name the document defines; if `GuardShare` maps to `share_id` and
   `GuardAuth` maps to `bearer_auth`, then the operation's `security` list _is_
   the record of which kind, once D8 holds. The extension would duplicate it.
3. **The mode guard never needed it, which is the tell.** `GuardReadOnlyMode` is
   not authentication, and it is not in `security` — it rejects with `405` and is
   documented as a `405` response (M2). If the one constraint that cannot be a
   credential needs no extension, the credential ones do not either.

So: no `x-picasu-*` extension is added, and the rules below are written against
`security(...)` alone. What remains open is not the shape of the mechanism but
the content of `securitySchemes`, which does not exist in the document yet — see
the scheme-location open decision under section D.

**The mode guard is not authentication and never appears in `security`.** It
rejects with `405` and `ErrorKind::ReadOnlyMode`, and it is documented as a `405`
response (M2). A client cannot satisfy it with any header or token, so a
`security` entry for it would tell a generated client to send something the
server ignores and leave it believing it is permitted when it is not. That is the
clearest sign it belongs to another class, and it is why no extension was needed
for it either.

**Explicit re-authentication gets its own scheme, not an extension.** "Present
the account password again" is not one of OpenAPI's five scheme types
(http/bearer, apiKey, oauth2, openIdConnect, mutualTLS), and the earlier
recommendation was to keep it out of `security` because a wrong scheme is worse
than an absent one — a client that believes it may send a header we ignore is a
client that believes it is authenticated when it is not. Under the decision above
that is still true of the _location_ of the credential, so it became an open
question about the scheme's content rather than about `security` itself: a
re-authentication scheme is declared, it is `apiKey`, and D5 checks that every
route carrying the re-authentication guard declares it (D6 checks that it is never
alone). What is still undecided is where each scheme says the credential travels
— see below.

Public operations (`renew_hash_token`, `renew_timestamp_token`, the login and
page routes) carry no authentication guard and must declare no `security`; the
rules below are written so that neither direction fires on them.

### A — the annotation's shape

Per annotation, no cross-file knowledge. The "no false positives found against the
current tree (63 annotations)" calibration below was written before A4 and A5
were run, and it was wrong: those two fired 50 and 8 times. What survives it is
recorded per rule in the progress notes.

| #   | assertion                                            | why nothing else enforces it                                                                                      |
| --- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| A1  | no `path = "..."` and no bare verb                   | the convention exists only in review; nothing rejects a re-introduced duplicate                                   |
| A2  | `responses(...)` present and non-empty               | utoipa invents no response, so the omission is silent                                                             |
| A3  | exactly one `tag`, from the closed vocabulary of ten | the vocabulary is documented in `docs/openapi-generator.md`; its checker was deleted                              |
| A4  | the handler carries a doc comment                    | `summary`/`description` are derived from it, so the omission is invisible except in the reference                 |
| A5  | the summary is one line                              | a multi-line summary splits the generated reference's headings — measured, not stylistic                          |
| A6  | no hand-set `operation_id`                           | utoipa derives it from the function name; a hand-set one is the only name nothing compares                        |
| A7  | no hand-set `summary` or `description`               | utoipa derives both from the doc comment; a hand-set one is the same prose twice, and nothing compares the copies |

**A3 has no exemptions, and the vocabulary absorbed the awkward case.** The two
test-only probes (`probe_record`, `probe_dupe_group`) are stripped from the
published document by `openapi_public`, so a reader of the reference cannot reach
them and a tag naming a public subject would misfile them. The vocabulary's tenth
entry, `internal`, says "outside the published API" instead: a rule with an
exemption is a rule with a way round it, and a tag in a list is a fact a reader of
the source can see where a reader looks. Every other rule applies to the probes as
before, because the contract tests read the _full_ spec rather than the public one.

**The tool holds no backend facts, and A3 is where that was nearly lost.** An
earlier version exempted the excluded routes by carrying
`CONTRACT_EXCLUSION_PREFIXES` as a copy in `openapi-sanity` (see the superseded
progress entry below). The copy was a second place to forget, and the rule around
it needed a route path, which is the shape of a fact belonging to the backend. The
durable rule for the tool is now written at the top of `utils/openapi-sanity`'s
module docs and in its README: **it holds annotation conventions, and if a rule
seems to need a route, a prefix, a config value or a feature name, that is a sign
it belongs in the backend or in a just recipe.** `TAGS` is the one set the tool
enforces that is written down elsewhere, and it is a convention — a table in
`docs/openapi-generator.md` — not a fact read out of the backend.

**A7 exists because A5's premise is false without it.** utoipa lets an
annotation set `summary` and `description` outright, and one annotation did
(`put/assign_album.rs`, `summary` and `description` carrying the same prose as
the doc comment beside them). While that is possible, "the first paragraph _is_
the operation's summary" does not hold, and A5 was reporting a defect the
generated document did not have. A7 closes it with A6's argument — a hand-set
name is the one nothing compares, and here the name is prose.

#### S1 — a tag follows the route family, and the rule needs no lists

A3 says every operation declares one tag from the vocabulary. It says nothing
about **which** tag, and a tag that names the wrong subject is invisible to every
check in this plan and to every tool outside it. S1 is the rule that says a data
operation is not filed under `pages`:

- a handler defined in `router/get/get_page.rs` is tagged `pages`;
- a handler defined anywhere else is not.

The reasoning, since it was not clear in review: **the generated reference groups
operations by tag, and `pages` is reserved for the SPA's HTML routes.** Those are
the routes that serve the shell a browser navigates to; the data operations are
what a client integrating against the API looks for. An operation tagged `pages`
files itself in a section no reader of the API looks in — the file is complete,
the rule set is satisfied, the document is valid, and the operation is simply
gone from where anyone would look for it. Nothing else catches it: A3 accepts
`pages` because it is in the vocabulary, `--check-openapi` only compares route
sets, and a document linter would accept it too, because the document is
well-formed and the tag is in the list.

**The rule needs no list of page routes and no list of data routes.** The route
family is read from the module the handler is defined in — one module is the page
family, every other module is not. A hand-maintained list of either kind is a
second place to forget, which is the failure mode the A3 exemption above was
replaced for: a rule with a list in it is a rule with a way round it. Adding a
new page module means adding a module, and the rule reads the module.

**Measured 2026-10-02 (calibration for S1, no source changed):**

| fact                                                     | count  |
| -------------------------------------------------------- | ------ |
| annotated handlers in `router/get/get_page.rs`           | **22** |
| … of those tagged `pages`                                | **22** |
| annotated handlers anywhere else in `backend/src/router` | **41** |
| … of those tagged `pages`                                | **0**  |

**S1 has zero findings today, in both directions**, and the earlier measurement —
22 in `get_page.rs`, all tagged `pages`, none elsewhere — is confirmed. (63
annotations in total, the count `the_router_tree_is_clean` pins.) A rule that
starts with zero findings is the good case: it is a regression guard, and its
value is that a future data operation tagged `pages` — easy to write by copying
the nearest annotation — is reported rather than absorbed.

**R1 is the same principle applied to the path.** A `POST` route lives under
`/post/`; a mutating verb elsewhere is either a mistake or a deliberate exception
a reader should see. Measured today: one route, `POST /get/prefetch?<locate>` in
`backend/src/router/get/get_prefetch.rs` — so R1 lands with one finding, in the
same change that decides which way it goes.

### B — what the annotation declares against what the route already says

The invariant: **the route wins, and anything the annotation restates must agree
with it.** This is the residue `rocket_extras` leaves, and it is source-only.

| #   | assertion                                                                                                                         | why nothing else enforces it                                                                              |
| --- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| B1  | every declared `params(...)` name corresponds to a route segment (`<name>`, `in: path`) or query binding (`?<name>`, `in: query`) | utoipa merges declared parameters into the derived document without checking they exist                   |
| B2  | a declared parameter's `required` equals `!argument_is_option` for the handler argument of that name                              | same                                                                                                      |
| B3  | a declared `request_body` schema equals the type the route's `data = "..."` binds                                                 | utoipa takes the declared schema and never compares it to the route's binding                             |
| B4  | a `Form<…>` binding declares `multipart/form-data`                                                                                | utoipa guesses `application/json` for a named non-primitive type, and nothing in the route says otherwise |

B1–B4 are enforced by `utils/openapi-sanity`, beside A and C1. The measures and
limits below are what they were calibrated against and what they do not read. Both
were established by measuring the tree before writing either rule.

**Measured 2026-10-03, before a rule was written (no source changed):**

| fact                                                                | count  |
| ------------------------------------------------------------------- | ------ |
| annotated handlers in `backend/src/router`                          | 63     |
| annotations declaring `params(…)`                                   | **1**  |
| … of those, the inline `("name" = Type, Location, …)` form          | **1**  |
| … of those, `params(SomeQueryStruct)` or a struct mixed with tuples | **0**  |
| `#[derive(IntoParams)]` anywhere in `backend/` or `frontend/`       | **0**  |
| annotations declaring a `request_body`                              | **24** |
| … declaring `request_body = Type`                                   | **24** |
| … declaring `inline(…)`, a `[T]` array or a `content = …` group     | **0**  |
| … where the declared type is exactly what `data = "…"` parses       | **21** |
| declared parameters read by B1/B2 (pinned)                          | **1**  |
| declared parameters _not_ readable in the inline form (pinned)      | **0**  |
| declared request bodies read by B3/B4 (pinned)                      | **24** |

#### B1/B2 read the inline tuple form, and the gap is pinned rather than assumed

utoipa accepts two spellings for a parameter in `params(…)`: the inline tuple
`("name" = Type, Location, …)` and a struct — `params(SomeQueryStruct)`, or a
struct mixed with tuples. The struct hides the name, the location and the type
behind a type the tool would have to resolve across files (find the
`#[derive(IntoParams)]`, read its fields, apply its `#[param(…)]` overrides). No
type in this repository derives `IntoParams`, so a resolver would ship untested
against real code.

**The gap is not left silent.** Every `params(…)` entry the rules cannot read is
counted in `HandlerSummary::unread_parameters`, and `the_router_tree_is_clean`
pins that count at **0**. The first struct form to reach an annotation makes the
pin fail, so the decision — build the resolver, or renegotiate the limit — happens
in review rather than as a quiet narrowing of B1 and B2.
`only_the_inline_parameter_form_is_read` pins the parsing itself.

A declared parameter whose location is neither `Path` nor `Query` is outside B1
too: a header or cookie is named nowhere in a Rocket route attribute, so there is
no route binding for it to disagree with.

#### B2 compares optionalities, because utoipa has no `required` key

The rule as specified reads "a declared parameter's `required`", and **utoipa 5.5
has no such key**: `("x" = String, Query, required = true)` is rejected by the
macro as an unknown attribute (`unexpected attribute: required, expected any of:
style, explode, allow_reserved, …`) — verified by compiling it against this
repository's own backend. The documented `required` is derived from the _declared
type_ alone: `Required::from(!type_tree.is_option())` in `utoipa-gen`'s
`src/path/parameter.rs`.

So the two optionalities that have to agree are the declared type's and the
handler argument's, and B2 compares those. That is the comparison the rule
describes, one step earlier, and it is non-tautological in both directions: a
declared `Option<T>` on a `T` argument tells a client it may omit a parameter the
route requires, and a declared `T` on an `Option<T>` argument tells it must send
one the route does without. The tool's module docs name the utoipa version and the
source line this rests on, so an upgrade that adds `required` is visible rather
than silent.

#### B3's two limits, and why each is a statement rather than an exemption

- **`request_body = Value` declares no constraint and is not compared.** It is
  utoipa's "any body" — both spellings publish an empty schema — so it says
  nothing a route could contradict. The asymmetry is deliberate: the other
  direction is still a finding, so `request_body = SomeType` on a route binding
  `Json<Value>` is reported. `a_body_the_route_parses_is_accepted` pins the limit.
- **A `Form<…>` binding is not compared.** `UploadForm<'r>` and
  `RegenerateThumbnailForm<'r>` carry `TempFile<'r>` and a lifetime, so there is
  no schema type an annotation could name and B3 has nothing to compare. What can
  be checked about a form body is the media type, which is B4.
- Types are compared by the **last segment** of their path, which is the name
  utoipa publishes the schema under, so `crate::model::album::SetAlbumTitle` and
  `SetAlbumTitle` agree.

`POST /get/prefetch` is what the first limit leaves standing: it declares
`serde_json::Value` over a `Json<Expression>` binding, so B3 does not fire and the
document is under-specified rather than wrong. That gap is **Q2**, in
`.plan/bug-prefetch-request-body.md` — a public-API decision, not a tool rule.

#### B4 found the two media types, and both were fixed in the change that wrote it

The measure pointed the other way from what was expected: B3 was going to be the
rule with findings, and **B4 is the one that fires**. Two routes bind a `Form<…>`
and declared `request_body = Value`, which utoipa published as `application/json`
with an empty schema — a multipart upload endpoint documented as a JSON one. Both
annotations were fixed in this change:

| site                                                | was                    | now                                                                    |
| --------------------------------------------------- | ---------------------- | ---------------------------------------------------------------------- |
| `backend/src/router/post/post_upload.rs:153`        | `request_body = Value` | `request_body(content_type = "multipart/form-data", content = Object)` |
| `backend/src/router/put/regenerate_thumbnail.rs:33` | `request_body = Value` | `request_body(content_type = "multipart/form-data", content = Object)` |

The document's diff is exactly those two media types — `multipart/form-data` with
`{"type": "object"}` in place of `application/json` with `{}` — and nothing else in
`backend/openapi.json` moved. The schema is an untyped object because itemising
the fields would need a `ToSchema` impl for a struct holding a `TempFile`, which
is out of scope for this rule; that belongs in whichever change gives the form a
schema.

### C — the handler body

| #   | assertion                                                                                                                                                                                          | calibration                                                                                                                                                                                                                                                                                                                                                                          |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| C1  | **a `GuardResult<…>` argument must have its rejection propagated**: the binding appears as `ident?`, is matched, or is forwarded. `let _ = ident;` and a binding absent from the body are findings | the tree has 52 `GuardResult<…>` bindings, all consumed as `let _ = ident?;`, so the conforming idiom is `let _ = ident?;` and the rule fires on the `84f29aa5` shape (`GuardResult<GuardTimestamp>` dropped without `?`). Zero findings today; it earns its place as a regression guard and must be proved by a mutation test (the deleted suite's `dropping_a_guard_result_fails`) |
| C1b | **a plain `Guard…` argument needs nothing in the body**, and must not be flagged                                                                                                                   | Rocket runs the guard during request handling and short-circuits on failure, so the handler legitimately never touches the value. Nine `_auth: GuardAuth` bindings across seven handlers are never touched. A rule that treated every guard type alike would report every one of those correct handlers as a finding                                                                 |
| C2  | every `pub fn generate_*_routes()` is mounted in `router/builder.rs`                                                                                                                               | not coverage — `--check-openapi` catches an unmounted group — but the gate's message ("documented operation, no route mounts") sends the reader to the document instead of to `builder.rs`                                                                                                                                                                                           |

#### C's known limitations, recorded so they are not rediscovered

- **`GuardResult<…>` is a crate alias** for `Result<T, AppError>`, and C1 matches
  the alias by name. A handler that spells the type out longhand would be
  invisible to it. Nothing does today; the same alias knowledge matters more for
  B2, which compares declared parameter types.
- **Guard classification is by type name, and that is enough only for C1/C1b.**
  `renew_hash_token` binds `TimestampGuardModified`, a plain Rocket guard that
  does not match the `Guard` prefix — harmless here, because these two rules care
  about the fallible alias and not about the class. The D-series cannot: it needs
  to know which guard _class_ a parameter is. That needs a real signal on the
  guard types (a `const fn` such as `is_authentication()`, or reading the class off
  the route attribute), not a name prefix. Recorded here as a design requirement
  for D.
- **A one-hop rebinding is reported, not accepted.** `let x = auth; … x?;`
  propagates in the author's intent and is a finding today, as is a `move` into a
  closure that does not itself propagate. Neither occurs in the tree; the safe
  direction is to report and let a human decide, but both should be pinned by
  fixtures so the behaviour is a decision rather than an accident.

#### A8 — guard bindings are named after their class (calibration first)

A8 is a convention, not a defect today: a guard parameter is named after the guard
class it binds, and `_`-prefixed when the handler discards the value
(`auth`, `read_only_mode`, `hash`, `share`, `upload`, `timestamp`, `_auth`). The
name is what a reader scans for when asking "which guard is this, and does the
handler use its value", and a name that says `auth` on a `GuardShare` binding
answers both questions wrongly at a glance. It is a naming rule, so it produces
findings, not crashes — which is why it is measured before anyone writes it.

**Measured 2026-10-02 over `backend/src/router` (no source changed):** 62 guard
parameters, which is the 52 `GuardResult<…>` bindings C1 is calibrated against
plus 9 plain `GuardAuth` bindings and one `TimestampGuardModified` that matches
neither pattern. **19 of the 62 would be findings** — roughly a third, across 12
files:

| binding                                           | count | A8 expects                                                                                     |
| ------------------------------------------------- | ----- | ---------------------------------------------------------------------------------------------- |
| `auth : GuardResult<GuardAuth>`                   | 18    | conforms                                                                                       |
| `read_only_mode : GuardResult<GuardReadOnlyMode>` | 15    | conforms                                                                                       |
| `_auth : GuardAuth`                               | 9     | conforms (discarded, so `_`-prefixed)                                                          |
| `read_only : GuardResult<GuardReadOnlyMode>`      | 5     | **finding** — should be `read_only_mode`                                                       |
| `auth : GuardResult<GuardShare>`                  | 5     | **finding** — should be `share`                                                                |
| `auth_guard : GuardResult<GuardShare>`            | 2     | **finding** — should be `share`                                                                |
| `guard_timestamp : GuardResult<GuardTimestamp>`   | 2     | **finding** — should be `timestamp`                                                            |
| `auth : GuardResult<GuardTimestamp>`              | 2     | **finding** — should be `timestamp`                                                            |
| `hash_guard : GuardResult<GuardHash>`             | 1     | **finding** — should be `hash`                                                                 |
| `hash_guard : GuardResult<GuardHashOriginal>`     | 1     | **finding** — should name the hash-original class                                              |
| `auth : GuardResult<GuardUpload>`                 | 1     | **finding** — should be `upload`                                                               |
| `auth : TimestampGuardModified`                   | 1     | out of scope — not a `Guard…` type, and a rule that needs the class cannot name it (see above) |

So the cost of A8 is known before it is written: **19 renames across 12 files,
zero rule changes, and no behaviour affected** — a parameter name is local to
the handler. That is a review-sized diff rather than a defect, which is the
argument for writing the rule and fixing the names in the same change instead of
holding the rule back until the tree is tidy.

### D — the guard and `security` rules

Every rule below is written against the decision above: `security(...)` and
`securitySchemes` only, no `x-picasu-*` extension. "Credential guard" means one
of the authentication-class guards (`GuardAuth`, `GuardTimestamp`, `GuardHash`,
`GuardHashOriginal`, `GuardShare`, `GuardUpload`); the mode guard is not one of
them and never appears in `security`.

| #   | assertion                                                                                                                                          | direction that matters                                                                                                                      |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | a route carrying a credential guard has an operation that declares `security(...)`, and the schemes it names are the ones that guard class maps to | undocumented authentication: a reviewer reading the document cannot tell the route is guarded                                               |
| D2  | an operation declaring `security(...)` has a route carrying a credential guard (no security declared for a route that enforces nothing)            | the dangerous direction: the document claims a requirement the code does not enforce                                                        |
| D3  | _cross-reference_ — the credential guard's rejection is propagated, and that is **C1**, not a separate rule                                        | security declared, guard present, rejection dropped — the `get_rows` shape end to end, and C1 already catches the last third                |
| D4  | `securitySchemes` defines every scheme any operation references                                                                                    | a dangling scheme name is a document that no generator can render                                                                           |
| D5  | a route carrying the re-authentication guard declares **its own scheme**                                                                           | the dangerous direction: the operation silently needs a password the document never mentions, so a client cannot perform it                 |
| D6  | the re-authentication guard appears **with** a credential guard, never alone                                                                       | a password check without a token is either a second authentication factor or an unauthenticated endpoint, and only one of those is intended |
| D7  | every `POST`/`PUT` route carries at least one credential guard, or is listed as deliberately public                                                | an unguarded mutating route is a hole with no rule to find it, and the list is where a deliberate hole is written down                      |
| D8  | the credential set an operation declares in `security(...)` equals the credential set its route's guards provide                                   | a route with two guards and a document naming one, or the reverse: the document and the code disagree about who may call                    |

D3 used to be a rule of its own and no longer is. The obligation it described —
the guard's rejection must reach the response — is exactly C1, and two rules
asserting it would give one defect two findings and two places to change.

**D7 and D8 cannot be written before Q1 is answered**, because D7 either encodes
a hole or encodes a workaround and only the backend knows which. See below.

#### Q1 — are these five routes deliberately public?

**Q1** is a backend question, not a tool rule, and it is numbered so D7 can point
at it. It was put to the review as: are these five `POST` routes deliberately
public?

- `POST /post/authenticate`
- `POST /post/index/album`
- `POST /post/index/image`
- `POST /post/index/cancel`
- `POST /post/config/import`

> **Measured 2026-10-02: four of the five do carry a credential guard**, so the
> question as asked rests on a premise that does not hold.
> `index_album_handler`, `index_image_handler`, `cancel_album_index_handler` and
> `import_config_handler` each bind `_auth: GuardAuth`
> (`backend/src/router/post/album_index.rs:40,64,91`,
> `backend/src/router/post/import_config.rs:26`). A scan of every mutating route
> attribute in `backend/src/router` finds 26 — 13 `PUT`, 12 `POST`, 1 `DELETE` —
> and **exactly one** carries no credential guard at all: `POST /post/authenticate`
> (`post/authenticate.rs:20`), which is the login route and is public by
> construction. All 13 `PUT` routes carry one, which is the part of the original
> question that holds.

What Q1 still needs, narrowed to what is actually open:

1. **Is `POST /post/authenticate` public on purpose?** Almost certainly yes — a
   login route that required a token could not be used to obtain one. Confirming
   it is what turns a one-entry exception into a documented one.
2. **Where does the deliberate-public list live** — a named constant in the
   backend, a documented omission per operation, or a route-level comment the
   rule points at. **D7 is not writable until this is answered:** a rule that
   flags an unguarded mutating route either carries an exception list (a second
   place to forget, the mistake the tool's module docs warn about) or names a
   list the backend maintains (a backend fact in a tool that holds none).
3. **Whether the other four are guarded on purpose or by accident** — they are
   guarded today, and the review should know whether that is the design or a
   side effect of the index routes being added after the guard list was.

#### Open decision: where a scheme says the credential travels

`securitySchemes` does not exist in the document yet, and a scheme is not free
of content: `apiKey` must state `in: header`, `in: query` or `in: cookie`, so
each scheme is a claim about **where the credential travels**. The share, upload
and hash guards accept a credential in a header **or** in the query string —
`try_resolve_share_from_headers` and then `try_resolve_share_from_query` in
`backend/src/router/auth.rs:783,793`, and the upload guard's
`presigned_album_id_opt` query fallback at `auth.rs:444` — so a single
`apiKey in header` scheme would misdescribe them: a client generated from it
would believe the query form does not exist, and the query form is what the
frontend and `<img>`-style requests use. `GuardAuth` has the same shape, more
quietly: it reads `Authorization: Bearer …` and falls back to a `token` query
parameter (`auth.rs:224,235`), so even the bearer token is not header-only.

The options:

1. **One scheme per credential kind, each stating the location it actually
   accepts.** Most precise, and it means more than one scheme for a guard that
   accepts two locations, so the count grows with the surface.
2. **One `bearer_auth` scheme for all of them.** Simplest, one scheme, one
   `security` shape everywhere — and wrong for four of the six credential
   guards, none of which reads a bearer token: `GuardShare` reads `x-share-id` /
   `x-album-id`, `GuardHash` reads a hash of the path, `GuardUpload` reads share
   headers. A client generated from it sends `Authorization: Bearer …` and is
   rejected.
3. **Declaring the query variant as a second scheme**, so one guard with two
   locations has two schemes and the operation lists both — as `security`
   alternatives, which OpenAPI defines as OR.

**The pick is (3), with (1) as its special case.** It is exact and it is
standard: `security([{"share_id": []}, {"share_id_query": []}])` is the
specification's own way of saying "either location satisfies this", so the
document is honest and a generated client is told about the query form rather
than left to discover it. The cost is scheme count, and that cost is paid in a
list a reader can read rather than in a rule a checker has to approximate. (2) is
rejected because its simplicity is purchased with a claim the server does not
honour, which is the same argument that decided against the extension. Nothing
here is implemented: D4/D5/D8 wait on it, and the schemes are added to
`ApiDoc`'s `components(...)` in the change that adds them.

### Review rules — not tool rules

**V1 — a credential comparison is constant time.** This was D7, and it is not a
tool rule: no checker can see it, because a constant-time comparison is a
property of the operation the code performs, not of its shape, and the tool reads
shapes. `update_password_handler` compares with `!=` (`edit_config.rs:157`),
which is not constant time; the guard that replaces it must not carry the same
habit forward. It is a review obligation, and it keeps the id it had so the
concern is not lost with the row it was in.

### M — the mode guard (`GuardReadOnlyMode`)

Not authentication, and it must never be documented as such. `read_only_mode` is
a **server-side setting** (`APP_CONFIG`, settable at startup through
`PICASU_READ_ONLY_MODE` — `backend/src/model/config.rs:403-408`, and through
`PUT /put/config` while the mode is off). The guard consults it and rejects with
**405 Method Not Allowed** and `ErrorKind::ReadOnlyMode` — a status that is not
an authentication failure, which is the clearest sign it belongs to another
class. Handlers consume it as `GuardResult<GuardReadOnlyMode>` and immediately
`let _ = read_only_mode?;`: the value is a unit struct, so the guard's only
effect is the rejection.

So the guard is the route's own declaration that **this route mutates state**,
and the setting decides whether that is currently permitted. Two consequences for
the document: the requirement is a documented `405`, not a credential; and a
client cannot satisfy it with any header or token, so putting it in `security`
would tell generated clients to send something the server ignores and believe
they are permitted when they are not.

| #   | assertion                                                            | calibration                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| --- | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| M1  | every route with a mutating method (`PUT`, `POST`) carries the guard | **13/13 PUT and 12/12 POST routes carry it, except six**: `POST /post/authenticate` (login), `POST /get/prefetch` (a POST that only reads), `POST /post/renew-hash-token` and `POST /post/renew-timestamp-token` (mounted from `auth.rs`), `POST /post/config/import` and `POST /post/index/cancel`. `DELETE` has one route, `DELETE /delete/delete-data`, which carries it. An earlier measurement of "zero exceptions" was wrong: it counted files rather than handlers and missed the `_auth: GuardAuth` bindings. **The exception list needs a decision — see Q1** — so M1 is not built until then |
| M2  | a route carrying the guard documents a `405` response                | **16 findings today.** Exactly one operation documents it — `POST /post/rebuild`, which established the convention as `(status = 405, description = "Read-only mode")` — while the other 16 mutating routes can answer 405 and list only 200/400/401                                                                                                                                                                                                                                                                                                                                                   |
| M3  | the guard never appears in `security(...)`                           | no findings today, because no operation declares `security` at all                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| M4  | the guard's argument is propagated with `?`                          | C1 restated for this class, so the rule reads where the class is described                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |

**M1's exception list is empty today and will not stay empty.**
`.plan/bug-readonly-lockout.md` exists because `PUT /put/config` — which
carries the guard — is also the only route that can set the mode to `false`, so
once the mode is on, the API cannot lift it. The fix introduces a
re-authentication route that must stay reachable precisely because the mode is
on; that route is the first named exception to M1, and the exception lives in
that plan rather than here. `PUT /put/config/password` is the open question: it
carries the guard today, and the same argument applies to it.

### E — deferred, and why

- **A crate-wide function-signature index** to enforce "a handler holding
  `GuardReadOnlyMode` must not call a function taking `&mut Config`". This is the
  one rule that genuinely needs cross-function analysis (an index of
  `fn → parameter types` by walking the crate's modules; no call graph needed).
  It is deferred because it needs a short, explicit list of mutating entry
  points, and without one it will produce false positives on every save helper.
- **A document linter** (Spectral or Redocly) for the document-shaped rules the
  deleted checker used to carry: tag vocabulary, `operationId` uniqueness and
  stability, `$ref` resolution, orphan schemas, descriptions. Until one is
  adopted, A3 and A6 are the only tag/operationId coverage this repository has.

### Where this runs

As a standalone tool, `utils/openapi-sanity`, not as build warnings and not as
backend tests. Both of those were tried: the deleted crate emitted a build
warning and nothing read it, and the rules as backend tests only ran in CI,
because the pre-commit hook runs `just backend-check` and `just openapi-check`
for a `backend/` change but not the test suite. The tool is the first phase of
`just openapi-check`, so it runs on the same `backend/` change that can introduce
the defect. It reports one `file:line: message` per finding, a summary line
naming the count, and exits non-zero on findings — the reporting contract the
other two `openapi-check` phases already use.

`just openapi-sanity` runs it over `backend/src/router`; `--source-root <dir>`
points it at any other tree. The phase also passes `--expect-at-least 60`, a floor
on the annotated handlers the scan must see before a clean run means anything: a
walk that stopped descending reports the same emptiness as a clean tree, so a
short scan fails instead of reporting clean. 60 is headroom below the tree's 63,
so a new handler does not break the gate. Rules live in the crate's `src/lib.rs`
so the CLI and the tests in `tests/openapi_annotations.rs` check the same code, and
`syn` with `proc-macro2/span-locations` is a normal dependency of the tool rather
than a dev-dependency of the backend. It stays out of the production binary either
way. The scan support for the two rules in section C is already ~400 lines, and A,
B and D add to the same walker rather than to a new one, so the whole list lands
in one tool.

**The name is reused deliberately.** The crate is named after the analyzer deleted
in `e1ec74da` because it does the same job — a source-level gate on these
annotations — but it is a new tool with a new rule set. None of that crate's rules
come back: no tag policy, no `AUTH_POLICY` table, no route-set rules, no
source/spec comparison. Its rule set is this plan, and of it A1–A7, B1–B4 and the
handler-body rules of section C are implemented.

### Sequencing and acceptance

1. **C1** first: it is the only rule with a proven incident behind it, and it
   ships with a mutation test that fails when the rule is removed.
2. **A1–A6** as one module: mechanical, calibrated against the tree, and they
   fail the branch if any of the 63 annotations regresses.
3. **B1–B4 landed** (see the progress note below), then **D1–D6** — the
   `security(...)`-versus-extension question is
   settled (see the decision above), so what they wait on is the schemes being
   registered in `ApiDoc` and the scheme-location open decision. **M1, M3, M4**
   land with them — all three are green today — and **M2 lands with the 16
   missing `405` responses added in the same change**, because a gate that starts
   with sixteen findings is a gate people learn to ignore. **D7, D8, A8, C3, S1,
   R1** follow once Q1 is answered; A8 and R1 are written with the renames they
   report, in the same change.
4. **C2** last; it is a message-quality improvement over a gate that already
   catches the condition.

Acceptance: every rule has a test that fails when the rule is deleted (mutation
style, no exceptions); the suite is green against the current tree with zero
findings except where a finding is the expected demonstration; `just test` and
`just check` are green; `docs/openapi-generator.md` states which rules are
enforced here and which are review-time, so the boundary is written down rather
than remembered.

## Progress

### 2026-10-03 — B1–B4 landed, and B4 found the two media types (uncommitted, for review)

Increment 3 of the sequencing: the four rules of section B, in the same tool and
the same attribute walk as A and C1. **The measure changed the plan twice**, and
both changes are recorded above under section B.

- **B2 has no `required` to read.** utoipa 5.5 rejects `required` in a parameter
  tuple as an unknown attribute (compiled against this backend to confirm), and
  derives the documented `required` from the declared type. B2 therefore compares
  the declared type's optionality with the handler argument's — the same
  comparison the rule describes, one step earlier, and non-tautological in both
  directions.
- **B4 exists because the measure found something B1–B3 do not describe.** Two
  routes bind a `Form<…>` and declared `request_body = Value`, so the document
  published `application/json` for two multipart upload endpoints. Both
  annotations now declare `request_body(content_type = "multipart/form-data",
content = Object)`, and `backend/openapi.json` moved in exactly those two
  places.
- **B3's limits are statements, not exemptions.** `request_body = Value` declares
  no constraint, so it is not compared — while a _named_ type on a route binding
  `Json<Value>` still is. A `Form<…>` binding is not compared, because a payload
  carrying `TempFile<'r>` has no schema type to name.
- **`POST /get/prefetch` is Q2.** Its `serde_json::Value` over a `Json<Expression>`
  binding is not a B3 finding under the `Value` limit, but the document is
  under-specified: a generated client cannot know what a valid prefetch body is.
  It is `.plan/bug-prefetch-request-body.md`, a public-API decision between
  publishing the filter grammar as a schema and describing it in prose.

**B1 and B2 are silent on the tree, B3 is silent, and B4's two findings were
fixed in this change.** The run over `backend/src/router` reports no findings in
63 annotated handlers. Mutation evidence, one rule at a time: removing B1 fails
`a_parameter_the_route_does_not_bind_fails`, B2 fails
`a_declared_optionality_the_argument_disagrees_with_fails`, B3 fails
`a_body_the_route_does_not_parse_fails`, B4 fails
`a_form_body_without_multipart_fails`, and each rule's conforming counterpart
stays green throughout.

Three new pins, because a scan that quietly reads less must fail rather than
report a clean tree: **1** declared parameter read, **0** declared parameters in a
form B1/B2 cannot read (the pin that forces the `IntoParams` decision), and **24**
declared request bodies. The count of unread parameters is the one worth keeping
an eye on: it is zero today and must stay zero until someone builds the resolver.

### 2026-10-02 — the rule index, the `security` decision, and two calibrations

Documentation only; no source changed, and the tool's rule set and its 23 + 5
tests are as they were.

- **Every rule has an id and a status.** The index sits directly under "The
  filter", which is where a reader arrives to ask "what does this plan actually
  require, and who enforces it today". Nine rules are enforced by
  `utils/openapi-sanity`; the rest are specified, blocked, a spike, a review
  obligation or a question for the backend. Ids are stated to be stable and never
  reused, because the reason for the index is to be able to say "A8" in review
  without ambiguity.
- **The `security(...)`-only decision is written down**, replacing the open
  question and the two-part design that left room for an `x-picasu-*` extension.
  The D table's rows are rewritten against it: D1 and D2 in the direction the
  decision makes checkable, **D3 demoted to a cross-reference to C1** (it
  described the same obligation, and two rules for one defect is two places to
  change), D5 now a scheme rather than an extension, D6 unchanged, and D7/D8 new
  and blocked. The constant-time comparison left the D list for the "Review rules"
  note as **V1** — it keeps the id D7 had so the concern is not lost, and the
  number is not reused.
- **Q1 is its own numbered task** because D7 is not writable without it, and
  because the answer is a backend fact rather than a tool decision. Measuring the
  routes before writing the question down found that four of the five named
  routes _do_ carry `_auth: GuardAuth`, so the question is restated around the one
  route that does not; the measurement is in the plan so it is not carried
  forward on a premise that does not hold.
- **S1 and A8 are calibrated before they are built** (no source changed):
  S1 — 22 handlers in `get_page.rs`, all 22 tagged `pages`, and 41 elsewhere with
  none tagged `pages`, so S1 starts at zero findings and is a regression guard.
  A8 — 62 guard parameters, 19 of which would be findings across 12 files, which
  is a review-sized rename diff rather than a defect. Both numbers are in the plan
  so that writing the rule is a decision with a cost attached, not a surprise.

### 2026-10-02 — the backend facts are out of the tool (uncommitted, for review)

**Supersedes the A3 exemption in the next entry down.** The exemption was the
right shape of _fix_ and the wrong _place_ for it: `EXCLUDED_ROUTE_PREFIXES` was a
copy of the backend's
`CONTRACT_EXCLUSION_PREFIXES`, and the rule around it had to read a route path to
apply. That is a backend fact living in a crate whose job is annotation
conventions, and a copy of a path list is a second place to forget — the gate
built on it would report a stale list rather than the truth.

So A3 is absolute again: every annotated operation declares exactly one tag from
the vocabulary, and the tenth entry `internal` is how an operation outside the
published API says so. `EXCLUDED_ROUTE_PREFIXES`, `AnnotatedHandler::route_path`,
`AnnotatedHandler::is_excluded`, `ROUTE_VERBS`, the `a3_excluded_prefixes.rs`
fixture and its two tests are gone; the two probes carry `tag = "internal"`.

The principle is now written where the next increment will read it: the module
docs at the top of `utils/openapi-sanity/src/lib.rs`, the tool's README, and the
comment on the `openapi-sanity` recipe in the justfile each say that the tool
holds conventions and no backend facts, and that a rule needing a route, a prefix,
a config value or a feature name belongs in the backend or in a recipe. That
sentence is the durable answer to an agent helpfully adding a backend detail to
the checker, which is the only way this mistake gets made.

### 2026-10-02 — A1–A7, the probe exemption, and `trace` (uncommitted, for review)

> **Superseded:** the A3 exemption below was replaced by the `internal` tag, and
> the code it describes no longer exists. A7 and `trace` in this entry stand.
> The entry is kept because the reasoning it records — a rule with an exemption is
> a rule with a way round it — is the reasoning the replacement rests on.

Follow-up to the A1–A6 increment below, from its review. Three changes, none of
which weakens a rule:

- **A3 exempted the excluded surface.** `EXCLUDED_ROUTE_PREFIXES` in the tool was a
  copy of the backend's `CONTRACT_EXCLUSION_PREFIXES`, and A3 skipped a handler
  whose route attribute was under one of them. The two test probes stopped being
  findings, so the tree was silent. The exemption was in the tool and not on the
  command line on purpose: a `--exclude-prefix` flag would spell the prefixes a
  third time, in the justfile, which is what the backend's constant exists to
  avoid. `a3_excluded_prefixes.rs` carried two exempt handlers _and_ a
  non-exempt one without a tag, and `a_tag_is_not_required_on_a_route_the_published_document_drops`
  asserted all of it in one render — the control is what stopped the exemption from
  being a way to switch the rule off. A handler with no route path was not exempt.
  **All of this was reverted**; see the entry above.
- **A7 — no hand-set `summary` or `description`.** Added because the review
  confirmed the A5 gap: `put/assign_album.rs` carried both, saying what its doc
  comment said, and while an annotation may set `summary`, "the first paragraph
  _is_ the summary" is false. `grep` over every `#[utoipa::path]` body in
  `backend/src/router` found **exactly one** override — `assign_album` at lines
  77 and 78 — so it was removed and its doc comment extended with the one clause
  the override carried that the comment did not ("or the destination album is a
  manual album"). Nothing else in the tree overrides either key, so nothing was
  deleted on a guess. `assign_album`'s `summary` in the document is now derived.
- **`trace` added to A1's verb list.** It is a Rocket verb and utoipa's
  `HttpMethod` accepts it as a bare token; the plan's list of eight omitted it by
  oversight. No annotation uses it, so the tree is unaffected and a restatement
  can no longer slip through the gap.

### 2026-10-02 — A1–A6 landed (uncommitted, for review)

Increment 2 of the sequencing: the six annotation-shape rules, as one module of
`utils/openapi-sanity` beside C1/C1b. Each is a small function over an
`AnnotatedHandler`; `scan_source_root` runs all of them, so the CLI and the
suite check the same code. One fixture per rule plus a conforming counterpart per
rule, all `include_str!`.

**Section A's own claim of "no false positives found against the current tree" is
wrong, and finding that is most of what this increment cost.** Three of the six
rules fired on the 63 annotations; the source was fixed in every case, no rule was
weakened:

| Rule | Findings on the tree | Fix                                                              |
| ---- | -------------------- | ---------------------------------------------------------------- |
| A1   | 0                    | —                                                                |
| A2   | 0                    | —                                                                |
| A3   | 2                    | resolved in review: the tenth tag, `internal`                    |
| A4   | 50                   | a doc comment on every annotated handler that had none           |
| A5   | 8                    | the first paragraph of eight doc comments reflowed onto one line |
| A6   | 0                    | —                                                                |
| A7   | 1                    | the `summary` / `description` override in `assign_album` removed |

A4 is the largest change in the branch and the most consequential: **49 of the
61 operations in `backend/openapi.json` had no `summary` before it.** The
annotation declares no `summary = "…"`, so utoipa derives the field from the
doc comment, and 50 of the 63 annotated handlers had none. 49 of the 50 handlers
that were missing one reach the document; the two test probes do not, which is
why the number is 49 and not 50. The document still looked complete, because an
operation with a `paths` entry, a `responses` map and a `tags` array is
indistinguishable from a documented one until a reader looks for prose.

A5's eight findings were the six multi-line summaries the document already had,
plus two in handlers whose operations the public-spec filter drops. The fix was
to move the wrapped sentences into a second paragraph rather than to lengthen one
line, so the `description` keeps everything the `summary` used to say. This is
also the rule with the one calibration worth recording:

- **`put/assign_album.rs` sets `summary = "…"` and `description = "…"` in the
  annotation**, which overrides the derivation A5 is about — so its multi-line
  doc comment produced a finding against a generated summary that was one line and
  correct. The doc comment was reflowed like the other seven, and the review that
  followed closed the gap with A7 rather than with an exception to A5: the
  overrides are gone and the doc comment is the single source.

**The A3 question is now answered** — by the tenth tag, as recorded in the review
entries above. `probe_record` and `probe_dupe_group` carry `tag = "internal"`, and
the rule has no exemptions.

Two smaller notes for the same review:

- **The vocabulary is a copy.** `TAGS` in the tool duplicates the table in
  `docs/openapi-generator.md`; the document cannot be parsed for it, because the
  document is generated from the annotations. The two are changed together, and
  both places now say so. It is a _convention_ written down in prose, which is a
  different thing from a backend constant, and the review that followed removed
  the one set that was the latter.
- **A1/A3 leave a residue.** `method(GET)`, `tags([…])` and `context_path` are
  spellings utoipa accepts for the same facts, none of which the rules reject and
  none of which the tree uses. `trace` was in this list until the review moved it
  into A1's verb list. The remainder is named in the tool's module docs, its
  README and `docs/openapi-generator.md` so the boundary is written down rather
  than remembered.

`backend/openapi.json` is regenerated in this change, and
`docs/openapi-generator.md` now states the enforced rules, the three-phase gate
and the corrected authoring steps.

### 2026-10-01 — the rules moved to `utils/openapi-sanity` (uncommitted, for review)

The two rules kept their behaviour and their calibration and moved house. The
backend placement was wrong for one reason: the pre-commit hook runs
`just backend-check` and `just openapi-check` for a `backend/` change and not the
test suite, so the rules only ran in CI. They are now `utils/openapi-sanity` — a
tool, first phase of `just openapi-check` — and `backend/Cargo.toml` no longer
carries `syn`.

Two holes in that arrangement were closed in the same change, because the phase
now runs where the rules used to be blind. The phase passes a handler floor
(`--expect-at-least 60`), so a scan that stopped descending fails instead of
reporting a clean tree — the CLI alone cannot tell that difference, only a floor
supplied from outside can. And the hook's `utils/` branch runs `just utils-test`,
so a change to the rules or the walk is exercised by its own suite before it
reaches CI rather than after.

The crate name is the deleted analyzer's name on purpose, which makes the old
"as backend tests" note above a historical record rather than the current state.

### 2026-10-01 — C1 and C1b landed (uncommitted, for review)

Increment 1 of the sequencing: the two handler-body rules. Implemented as
backend tests in `backend/src/tests/openapi_annotations.rs` (rules and
fixtures) and `backend/src/tests/openapi_annotation_scan.rs` (the `syn` scan),
with `syn` 3.0.5 and `proc-macro2/span-locations` added as dev-dependencies.
The scan reads `src/router` through `walkdir`; findings render as
`path:line: handler: what is wrong`.

Zero findings across the tree, and the calibration constants are pinned so the
scan cannot go quiet by walking less:

| fact                      | pinned value | where it is checked                                 |
| ------------------------- | ------------ | --------------------------------------------------- |
| annotations               | 63           | `guard_propagation_is_clean_across_the_router_tree` |
| `GuardResult<…>` bindings | 52           | same test                                           |
| plain `Guard…` bindings   | 9            | same test                                           |

**Correction to C1b's calibration:** the tree has **nine** plain-guard bindings,
not five — `album_index.rs` ×3, `edit_config.rs` ×2, and one each in
`rebuild.rs`, `import_config.rs`, `get_fs_completion.rs`, `get_album_index.rs`.
All nine are `_auth: GuardAuth` and none of them is touched in the body. The
conclusion of C1b is unaffected; only the count was wrong, and the test pins 9.

**Open reading of C1, taken literally and worth a review decision:** the
propagating positions are the operand of `?`, the scrutinee of `match` /
`let` / `if let`, an argument of another call, and a returned value (including
the trailing expression). A two-step `let carried = auth; … carried?;` is
therefore a finding even though a human would accept it; neither it nor any other
shape occurs in the tree today. A `move` into a closure is a different case and is
**accepted** when the closure propagates (`spawn_blocking(move || { … auth?; … })`),
because the walker recurses into closure bodies. Both are pinned as tests:
`rebound_guard_result_is_reported` and `guard_moved_into_a_closure_is_accepted`.

Guard classification is name-based: `GuardResult<…>` carries the obligation, any
other type whose last segment starts with `Guard` carries none, and anything else
is out of scope. Note that `renew_hash_token` binds `TimestampGuardModified`,
which is a plain Rocket guard that does **not** match the `Guard` prefix. That
is harmless for C1/C1b (it is not a `GuardResult`), but the D-series rules will
have to identify guard classes by something other than a name prefix.
