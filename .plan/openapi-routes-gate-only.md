---
status: done
type: feature
priority: high
area: backend
---

Reduce the OpenAPI gate to generation plus a route-set comparison.

## Final state

The gate has three phases, all wired into `just openapi-check`:

| Phase                  | Checks                                                          |
| ---------------------- | --------------------------------------------------------------- |
| `openapi-sanity`       | Source annotations against the routes beside them               |
| `openapi-json-match`   | `backend/openapi.json` against the spec generated at build      |
| `openapi-routes-match` | `--check-openapi`: mounted routes against documented operations |

The route-set comparison is asymmetric: a mounted route must be documented, and a
documented operation must be mounted unless it is feature-gated with the feature
off. `backend/src/spec_path.rs` translates between the mount table and the
document; `backend/src/openapi_parity.rs` performs the comparison and applies
`CONTRACT_EXCLUSION_PREFIXES`. Reporting lists waived feature-gated operations and
contract exclusions as separate bullet lists, and the summary line counts only
matched routes:

```text
check-openapi: 61 routes matched to spec.
```

`GET /get/test/` probe registrations are gated behind `#[cfg(test)]`, because the
public document never carried them and a shipped build would otherwise fail the
gate.

Superseded work: an earlier revision of this plan deleted `utils/openapi-sanity`
and the source rules with it. The crate and its rules were restored on
`openapi-annotation-only`, which is why the gate has three phases rather than two.
Coverage for the source rules is specified in `openapi-annotation-checks.md`.
