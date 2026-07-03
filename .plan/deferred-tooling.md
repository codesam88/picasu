---
status: idea
type: feature
priority: medium
area: testing
---

Further validation & QA directions.

Pick individual items from the list. Elaborate and create new 'open' issue for the detailed plan.

- **cargo bench** to measure and notice regressions
  - indexing performance
  - view cache performance
- **telemetry:** storage, items, view cache, rocket handler latency
- **Frontend size** optimize frontend/download size
- **API fuzzer** to explore unexpectedly exposed endpoints, missing authentication, concurrency issues
- **knip** — periodic dead-export sweep, not in precommit
- **Zod (or valibot)** — investigate for runtime validation of API responses; TypeScript types don't catch backend schema drift at runtime
- **MIRAI** https://github.com/endorlabs/MIRAI/tree/main

- add cargo geiger report scan / eval
- upgrade vite-pwa-plugin? https://github.com/vite-pwa/vite-plugin-pwa/issues/933
- add ossf scorecard, cli + ci: https://scorecard.dev/
