---
status: open
type: feature
priority: medium
area: backend
---

## Problem

The generated spec declares `securitySchemes: {}` — nothing. Every
downstream consumer (Schemathesis auth inference, code generators,
docs) must hand-wire credentials that the server already understands
(cookie `jwt`, bearer prefetch/asset tokens, share headers). The
Schemathesis pilot paid this cost directly: all auth hand-configured
because auto-detection had nothing to read.

## Goal

The generator emits the real schemes (cookie, bearer, share-header
API keys) and each operation references the ones its guards enforce,
so tooling can infer authentication from the contract.

## Steps

1. Declare the schemes in the generator (`backend/src/openapi.rs` or
   per-route annotations) matching enforced reality — cookie for
   user/admin, bearer for timestamp/hash presenter flows, share
   headers where `GuardShare` reads them.
2. Reference them per operation; regenerate and confirm
   `just openapi-check` green.
3. Re-run the pilot's auth wiring against the new spec to confirm
   inference works (scratch only).
