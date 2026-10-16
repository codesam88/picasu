---
status: done
type: feature
priority: high
area: backend
---

## Notes

`auth_key` leaves `config.toml`: it lives in `<DATA_HOME>/auth/secrets.json`
(`0600`, atomic writes), is never exported, and the API offers only reset
(generating a new strong random key) — never read or set.

## Decisions

- `AppConfig.auth_key` stays as the runtime cache; all JWT readers unchanged.
  Only persistence/ingress moves: boot reads the secrets file (a legacy
  `[secrets] auth_key` is ignored — no migration), and `PICASU_AUTH_KEY`
  still wins in memory without being persisted.
- Reset is `POST /post/config/auth-key/reset` (admin `GuardAuth`, empty
  body) returning `{ hasAuthKey: true }`. Rotation kills all sessions by
  construction (signing key changes).
- `GET /get/config/export` redacts `authKey`; `PUT /put/config` rejects any
  `authKey` presence with 400 (host-only table).
- Settings UI: the key set/clear controls become a reset button + status
  line; `AdvancedConfig` stops sending `authKey`.
- `docs/config.md` documents the new layout; `backend/openapi.json` +
  reference regenerated.

## Implementation steps (TDD red-first, one slice per commit)

1. Secrets store (`auth/secret_key.rs`): load/save/migrate unit tests.
2. Boot wiring + `config.toml` stops persisting the key (legacy adopt,
   round-trip test updates).
3. Export redaction + PUT rejection + reset endpoint with endpoint tests.
4. Frontend reset control + api fn with vitest.
5. Docs + OpenAPI regen + full gates.
