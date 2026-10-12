---
status: open
type: bug
priority: low
area: backend
---

## Problem

API routes with a missing required query parameter (seen on
get-data, get-rows, get-scroll-bar) fall through to the SPA catch-all
and answer `200 text/html` instead of 4xx/JSON. Found by the
Schemathesis pilot (cluster F); verified manually (`?timestamp=1`
without `end` → 200 HTML; full params → normal 401 JSON).

## Goal

Missing required query params on API routes are rejected as 4xx with
a JSON body; the SPA fallback serves only non-API paths.

## Steps

1. Pin the behavior per affected route (missing param → 4xx JSON).
2. Fix route mounting/order so API paths never reach the catch-all.
3. Re-run new tests + full backend suite.
