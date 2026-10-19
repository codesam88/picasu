---
status: backlog
type: chore
priority: low
area: frontend
---

# Frontend consolidation and cleanup pass

Opportunistic cleanup vehicle — items execute as ride-alongs when
adjacent code is next touched, or as one dedicated pass if scheduled.
Re-verified against the tree 2026-10-19 (the 2026-07-01 survey re-checked
item by item; items absorbed from the auth-enrollment re-review are
labeled where they originate).

## 1. Dead `getIsolationIdByRoute` indirection (lowest risk, do first — re-verified)

17 `.vue` files still call `getIsolationIdByRoute(route)`
(`script/utils/getter.ts:8`), which has always returned `'mainId'` since
the route-flattening refactor (`'subId'`, the only other historical
value, has no producers left).

Fix: inline `'mainId'` at call sites, delete the function, and drop
`'subId'` from the `IsolationId` union in `type/types.ts:56`. Keep
`'tempId'` and — an update vs the 2026-07-01 note — `'shareId'`
(used across 17 files, legitimate member).

## 2. Dead API bindings: `exportConfig` / `importConfig` (from R26 tail — verified)

`api/config.ts:39-52` exports `exportConfig` and `importConfig` with
zero importers repo-wide (verified 2026-10-19). Delete both and any
`AppConfig`/axios scaffolding only they use. Check `api/config.ts` for
further orphaned members while there.

## 3. T1 — auth util module consolidation (from auth review R30)

Five `script/utils/` modules, re-measured 2026-10-19: `passwordChange`
(18 src / 55 test, 1 caller), `registration` (66 / 235, 1 caller),
`logout` (29 / 89, 1 caller), `currentUser` (59 / 78, **4** callers),
`retryAfter` (80 / 191, **2** callers).

Merge toward two modules, keep suites as orchestration tests: saves
mostly file/import ceremony (~−20…−40 src, ~−60…−80 with test re-homing).
Constraints: the 235-line `registration.test.ts` stays — sole pin for
race-lost / throttle-vs-interceptor display contracts (no component
harness: `@vue/test-utils`/jsdom absent, vitest `environment: 'node'`);
`registration.ts` couples to `first-admin-registration.spec.ts` via an
inlined `RACE_LOST_NOTICE` copy — de-duplicate the constant first.
Effort ~2 units; not worth standalone scheduling.

## 4. T2 — bootGuard tripwire trim (from auth review R31)

`route/registerRoute.test.ts` S7–S10 carry in-code "R31 rewrite
candidate" marks (superseded by `bootGuard.test.ts` behavior tests), but
the settled r26 B3 decision keeps them as tripwires. Verified split:
only S9 (~−7 lines, pure dup of S8's first assertion) is freely
deletable; S7/S8 wiring pins are the only routes-side injection checks —
keep. Coverage fact to carry, not fix here: the change-password form has
zero e2e contact; `changePasswordForm.test.ts` remains its only check.
Effort ~1 unit for S9 + mark-comment cleanup.

## 5. `BaseModal` adoption for hand-rolled modals (re-verified)

`BaseModal.vue` is used by `ShareModalBase`, `UploadOptionsModal`,
`UserSettingsModal`. Still hand-rolling `<v-dialog>` + card scaffolding:
`AssignAlbumModal`, `EditBatchTagsModal`, `ShareDeleteConfirmModal`,
`EditTagsModal`, `ShareLoginModal` (new vs the 2026-07-01 list),
`AlbumInfoModal`. Medium effort (six templates); dedicated pass, do not
ride along.

## 6. Edit helpers split across two homes (unchanged)

Same category (PUT an edit, patch local store), two homes with no rule:
`api/edit{Flags,Rating,Tags}.ts` vs `script/utils/{editDescription,
createAlbums,quickEditTags}.ts`. Only worth doing while establishing a
convention going forward (e.g. `api/` as the single home); explicit
decision needed, else leave.

## Not consolidation targets (re-checked 2026-10-19)

- `components/Menu/MenuItem|MenuButton` family — still appropriately
  small and single-purpose.
- Pinia stores (33 files, mostly tiny `useXStore(isolationId)` factory
  stores) — idiomatic per-isolation-id design, not duplication.
- Backend-side review minors stay in their tracker (R28 residual typed
  knob helper / verb alias / status typing; R29's 1-line comment drift
  rides with R33 in `.plan/auth-enrollment-review.md`) — not frontend.

## Notes

- 2026-07-01: initial low-effort ad hoc scan requested by the user.
- 2026-10-19: full re-verification of every item against the tree;
  T1/T2/T3 absorbed from the auth-enrollment re-review
  (`.plan/auth-enrollment-review.md`, § Re-verification 2026-10-19) —
  the earlier dangling pointers now resolve here.
