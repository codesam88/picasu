---
status: backlog
type: chore
priority: low
area: frontend
---

Frontend tooling improvements:

1. **knip** — periodic dead-export sweep, not in precommit
2. **Dependency update cadence** — establish a regular cadence (npm-check-updates, Dependabot, or Renovate); small frequent updates cheaper than batched drift
3. **Zod (or valibot)** — investigate for runtime validation of API responses; TypeScript types don't catch backend schema drift at runtime
