---
status: backlog
type: chore
priority: low
area: testing
---

## Notes

The `catalog` checker (`utils/quint/coverage/checks/catalog.ts`) validates
`requirements.json` `tests` arrays only as non-empty strings for `verified`
requirements; it never checks the named tests exist. Two stale refs
(`shutdown::sigterm_exits_while_backstop_scan_is_running` in REQ-6/REQ-14)
passed the gate silently until found by inspection during the inductive
track's A5 reconciliation (removed there). Either validate test names
against the tree (module-path or path::fn resolution) or downgrade the
field's meaning in the schema so a passing gate is not read as existence.
