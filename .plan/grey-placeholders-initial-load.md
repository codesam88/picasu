---
status: done
type: bug
priority: high
area: frontend
---

**Grey placeholders on initial load** — `useElementSize` reports width 0 on first render; `usePrefetch` skips the fetch (guard: `windowWidth > 0`); the Home.vue watch then fires a row fetch before `prefetch()` sets the correct timestamp, so rows arrive stale and are discarded.

Root: `useElementSize` ResizeObserver fires after the first render tick, too late for the initial fetch path.

Fix: ensure `prefetchStore.windowWidth` is set before the first `fetchRowInWorker` call, or delay the Home.vue watch fetch until after `processPrefetchChain` completes.

### marked done - issue disappeared as of 03e9c654
