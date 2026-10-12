---
status: open
type: bug
priority: medium
area: backend
---

## Problem

Edit ops `edit_flags` / `edit_rating` / `edit_tag` answer 500
(`Failed to open tree snapshot`) for an unknown snapshot id
(`timestamp=0` or any garbage) instead of 400/404. Found by the
Schemathesis pilot (cluster H); reproduced manually. The spec even
documents 500 for these ops, so the contract currently accepts a
crash-shaped answer for bad input.

## Goal

Unknown snapshot id is rejected as client error (400/404); no 500 for
any `timestamp` value. Spec updated to match (drop documented 500 or
scope it to genuine storage failures).

## Steps

1. Pin the behavior: unknown `timestamp` on each of the three ops
   asserts non-5xx + JSON error body.
2. Fix at the snapshot-open path (validate before opening).
3. Re-run new tests + full backend suite; update spec annotations.
