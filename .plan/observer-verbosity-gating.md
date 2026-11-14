---
status: done
type: feature
priority: medium
area: backend
---

## Notes

Rename Cargo feature `observe` -> `task-observer`; gate extra observer stats
behind env-only `PICASU_OBSERVER_STATS` (default-disabled) + `--observer-stats`
CLI flag. Breach warnings stay on; recording stays always-on when feature
enabled.

### Decisions

- Feature rename: `observe` -> `task-observer` (kebab-case) in Cargo.toml,
  cfg attrs, openapi_parity KNOWN_FEATURES, comments.
- Opt-in literal `1` (like `PICASU_WEAK_PWHASH`).
- `drain_report`: table + info! only when `observer_stats()`; always reset.
- `emit_wrap`: skip release info! when flag false; debug demotion unchanged.
- `record`, `Timer`, rings, counters, `emit_breach` untouched.
- Init in `lib.rs::run()`: read env once, call `observe::set_observer_stats`.
- Stub mirrors `set_observer_stats`/`observer_stats` for smoke parity.

### Implementation steps

1. Rename feature (done).
2. Add AtomicBool + accessors in observe.rs and stub (done).
3. Gate drain_report/emit_wrap (done).
4. CLI flag + env bridge in main.rs (done).
5. Init in lib.rs (done).
6. Docs + plan notes (done).
7. Tests: help_lists_every_flag, observe_smoke parity, drain-gate test.
