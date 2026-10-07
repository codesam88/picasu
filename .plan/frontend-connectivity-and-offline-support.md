---
status: idea
type: feature
priority: medium
area: frontend
---

## Notes

### Problem statement recap

The previous backlog item covered image transmission retry policy for small images. But connectivity handling needs to be broader, especially for mobile usage where network goes up/down frequently (intermediate connectivity loss, switching between WiFi/cellular, airplane mode). Also want to consider offline support leveraging existing local image cache.

This is a larger design task than just tweaking retry logic.

## Goal

Create a design document outlining an overall strategy for handling connectivity changes in the frontend: detection, retry policies per request type, offline mode, cache utilization, and UX. Keep it pragmatic and incremental.

## Design areas to consider

1. **Connectivity detection**
   - Use `navigator.onLine` as coarse signal.
   - Listen to `online`/`offline` events at window level.
   - Consider more granular detection (e.g. periodic reachability checks to backend health endpoint) for mobile networks that may appear connected but have no route.
   - Debounce/throttle state changes.

2. **Request classification by criticality**
   - Asset/image fetches (thumbnails, full images): non-critical, can be deferred/retried. Can show placeholders/blank states.
   - Data fetches (timeline, album list, metadata): important for navigation; may need to show cached data if available.
   - Mutations (upload, edit, delete, move): critical user actions. If offline, queue them for later (background sync) or show explicit error with retry/manual action.
   - Prefetches: low priority, drop if offline.

3. **Retry policy by category**
   - Image/transmission failures (no response): selective bounded retry with backoff+jitter as before. Distinguish from HTTP errors.
   - For mutations when connectivity returns, need idempotency keys or conflict handling.
   - Consider circuit breaker pattern for repeated failures to a specific endpoint.

4. **Offline support & caching**
   - Leverage existing blob caches: in-memory (`blobCache`) and Cache API (`img-blob-cache-v1`) for thumbnails/full images. These already persist across sessions.
   - Consider Service Worker for fine-grained offline control and asset routing (but SW is currently blocked in tests; need to be careful about interaction).
   - For API data (timeline state, album structure), consider using IndexedDB or Cache API to store responses. But timeline can be large and mutate frequently.
   - Detect when offline: serve from cache when available; show offline indicator.
   - Stale-while-revalidate vs cache-first for different content types.

5. **UX considerations**
   - Global offline banner/indicator when `navigator.onLine` is false.
   - Skeletons/placeholders for loading; distinguish "loading" vs "failed to load" vs "offline - not cached".
   - For failed images: current blank state is fine; maybe show subtle offline icon if offline and not cached.
   - Mutations: if attempted while offline, save to outbox (IndexedDB) and retry on reconnect, or block with message.
   - Avoid retry storms when coming back online (coalesce requests).

6. **Mobile-specific concerns**
   - App may go to background/foreground - need to handle reconnection on resume.
   - Network type changes affect quality; not directly handled but retries help.
   - Limited storage/battery - be conservative with prefetching when on cellular or low battery (if we can detect).
   - iOS Safari quirks with Cache API / Service Workers.

## Incremental approach

This is large. Suggest splitting into phases:

**Phase 1 (immediate):** Refine image retry policy (the backlog item) - bounded retries with backoff+jitter, correct classification, tests. Low risk.

**Phase 2 (core connectivity):** Global connectivity state (Pinia store), listen to online/offline, show indicator, adjust behaviors (e.g. don't prefetch when offline).

**Phase 3 (offline data):** Cache critical API responses (e.g. current view's timeline data) for offline read. Maybe use existing snapshot mechanisms?

**Phase 4 (mutation queue):** Outbox for offline mutations with background sync when possible. Complex due to conflicts.

**Phase 5 (Service Worker):** Consider full offline experience. Requires careful design given test constraints.

## Testing strategy

- Unit tests for connectivity store, retry logic with backoff.
- Playwright tests simulating offline/online transitions (using `page.context().setOffline()` in Chromium, or proxy manipulation).
- E2E scenarios: go offline, navigate cached views, come back online; mutations while offline.

## Open questions

- How much offline support is desired? Read-only offline access to cached images/albums is valuable on mobile; mutation offline is much harder.
- Should we distinguish "offline" from "slow connection"? Sometimes useful (adaptive quality).
- Interaction with existing Service Worker if present? Current code blocks SW in tests.
- What about uploads in progress when connectivity drops? Need to handle aborts/retries.

## Next steps

- Review existing codebase for connectivity/cache patterns (any existing online/offline handling? check stores/components).
- Decide scope for first increment (likely just the refined retry policy + basic connectivity detection).
- Create more detailed design for chosen phase.
