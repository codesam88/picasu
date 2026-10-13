# Rocket request guards

Rocket 0.5's mechanism for **declared per-request validation policy**: any
type implementing the `rocket::request::FromRequest` trait is a request
guard, guards appear as ordinary handler arguments, and the framework
runs every guard **before** dispatching to the handler — a handler runs
only if all of its guards pass. The design consequence worth internalizing
is that the route signature becomes the authorization manifest: what may
pass, in what order, and with what failure semantics is all visible in one
place.

## Baseline (the obvious third)

Three pieces. One: the `FromRequest` trait — `type Error: Debug` plus one
async method `from_request(&Request<'_>) -> Outcome<Self, Self::Error>`;
implementations must be decorated `#[rocket::async_trait]`. Two: guard
identification by position — any handler argument type **not named in the
route attribute** is a guard (a `<param>` path parameter is not; the type
must implement `FromRequest`), and an arbitrary number may appear. Three:
the returned `Outcome` determines request fate, and it has three variants
whose differences are the whole subtlety (next section). Rocket's provided
`FromRequest` set — always-succeeding extracts (`Method`, `&Origin`,
`&CookieJar`, `&Config`, …) and status-forwarding extracts (`&Host`,
`&ContentType`, `&Accept`, `IpAddr`, `SocketAddr` — each forwards a 500
when the value is absent or unknown) — covers the trivial cases so custom
types are written only for policy.

## Error vs. Forward — rejection is not un-routability

The `Outcome` variants look symmetric and are not; misreading them is the
standard request-guard mistake.

- **`Error(Status, E)` — the request fails.** Rocket answers with that
  status via the designated error `Catcher`; no other route is tried.
  This is the right outcome when the request is _rejected by policy_.
  The canonical upstream shape is the `ApiKey` guard: a missing header →
  `Outcome::Error((Status::BadRequest, ApiKeyError::Missing))`; a present
  but invalid key →
  `Outcome::Error((Status::BadRequest, ApiKeyError::Invalid))`. The
  catcher tier renders the failure, and the handler **only ever sees a
  valid key**.
- **`Forward(Status)` — try the next route.** The request moves to the
  next matching route in rank order; only when no further route matches
  does the catcher for `Status` answer. This is the right outcome when
  this route is _not the handler for this request_ — a tiered `dashboard`
  route pair (`rank` 1 for admins, `rank` 2 for users) expresses the
  fork in the route table without a single conditional in a handler.
- **Catchers are where a rejected request's response is produced**, and
  they apply to both terminations (a failed guard, or a forward with no
  route left). Whatever message quality the client sees sits there, not
  in the guard.

## `Option` and `Result` arguments (and the silent-`None` footgun)

Guards can be softened — at the type level, in the handler signature —
through two provided self-impls:

- **`Result<T, T::Error>`** converts `Success` → `Ok`, `Error` → `Err`
  (handler inspects the guard's own error value); a `Forward` is **not**
  caught — the request is forwarded again with the same status.
- **`Option<T>`** converts `Success` → `Some`, and converts **both**
  `Error` and `Forward` → `None`; the impl itself always succeeds.
- **`Outcome<T, T::Error, Status>`** (as a `FromRequest` impl) passes the
  full outcome through for handlers that must distinguish every case.

The `Option` footgun is upstream-documented and severe: requesting
`Option<T>` silently converts a policy `Error` into `None`, so a guard
built to hard-fail becomes a non-fatal signal the handler has to interpret.
A guard whose absence "should never happen" read as `None` is a policy
change hiding behind convenience — use `Option` only where absence is a
first-class input state, and reach for `Result` (or the raw `Outcome`)
when the rejection's error value matters.

## Order is a design decision

Guards fire **left-to-right in declaration order**, and outcomes
short-circuit: the first `Error` skips the remaining guards. Two
consequences. Failure-response identity: with multiple guards able to
fail, the first one to fail decides the response — order defines which
denial the client sees and what distinguishing information survives.
Cost: expensive guards placed after cheap ones are rarely reached, but a
guard composed of other guards re-enters them; order interacts with the
caching below rather than replacing it.

## Request-local state for composed guards

Guards that compose other guards re-run the work per invocation:
upstream's canonical example is a `User`/`Admin` guard pair where a
non-admin's request queries the database **twice** — once through `Admin`
invoking the `User` guard, once through `User` invoked directly.
`request.local_cache_async(...)` runs the computation **at most once per
request**, no matter how many times the guard fires while routing; cached
data is owned by the request cache, which lets guards hand out **borrowed**
values (`&'r User`). Upstream guidance: guards doing database or
external-service work that may run multiple times during the routing of a
single request should use the request-local cache. `Infallible` as the
`Error` type is the companion idiom there — the guard expresses policy
only through the outcome, never a handler-visible error, and the double
query (or re-fetch, or re-decode) that per-invocation guards imply is
collapsed to once per request.

## In-depth sources (one line each)

- `rocket::request::FromRequest` API docs — read for the outcome
  semantics, the full provided-impl behavior table (`Option`/`Result`
  conversions, forwarding extracts), the `ApiKey` example, and
  request-local state:
  <https://docs.rs/rocket/0.5.1/rocket/request/trait.FromRequest.html>
- The Rocket guide — read for the request-guards chapter, plus routing
  (ranks, route matching), catchers, and request-local state:
  <https://rocket.rs/v0.5/guide/>
- Canonical API reference — read when the docs.rs render lags the
  release: <https://api.rocket.rs/v0.5/rocket/>
- The Rocket repository — read the `request` module source for
  micro-behaviors the docs summarize
  (<https://github.com/rwf2/Rocket>).

Guards as declared policy discipline: a guard impl should be reviewable
in isolation — every failure mode maps to exactly one outcome, the
response a failure produces lives in the catcher tier rather than in the
guard, and anything the guard re-derives per invocation is either cheap
or placed behind the request-local cache.
