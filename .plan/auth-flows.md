---
status: in-progress
type: meta
priority: high
area: meta
---

# Auth flows tracker

Top-level tracker for all user-authentication flows (reviewed
2026-10-13). Authentication holds first assurance priority
(`docs/test-strategy.md`): each flow below moves through review of
the desired functionality, then — typically in its own plan —
detailed sequence flows, implementation, tests, and review. The
enrollment slice is the active work; everything else queues behind
it with its desired behavior already recorded here.

States: `done` (reviewed, working, pinned by tests), `doing`
(covered by active work), `todo` (accepted, plan pointer set),
`elsewhere` (owned by another ticket).

| #   | Flow                                 | Now                                | Desired (reviewed)                                                                                                     | Plan                        | State     |
| --- | ------------------------------------ | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | --------------------------- | --------- |
| 1   | Legacy migrate login                 | backend + UI fallback              | Unchanged: first login with the config password migrates the claimed id (object path) or `admin` (string path)         | existing tests              | done      |
| 2   | Login / logout                       | backend + UI                       | Id + password mints a 14-day token; logout clears the client cookie (server token lives to expiry — accepted)          | existing tests              | done      |
| 3   | List / promote / demote              | backend + UI + zero-admin 409      | Admins manage roles; any zero-admin transition refused (409) on both layers; sole-admin row locked in UI               | existing tests              | done      |
| 4   | Self password change                 | backend + UI                       | Own password with old (non-admin) or without (admin); new passwords under the default policy                           | existing tests              | done      |
| 5   | First-admin bootstrap (online)       | backend open mode; flag/UI missing | Empty store + `--bootstrap-admin` (+freshness) opens enrollment; first create mints the first admin; closed otherwise  | auth-flows-enrollment.md    | doing     |
| 6   | Admin creates user                   | backend done; UI missing           | Admin-create form in the panel (id, password, admin flag)                                                              | auth-flows-enrollment.md §6 | doing     |
| 7   | Admin sets user password             | backend done; UI missing           | Set-password control in the panel (no old password needed)                                                             | auth-flows-enrollment.md §6 | doing     |
| 8   | Open registration                    | backend done; UI missing           | Toggle on: anyone registers a non-admin account                                                                        | auth-flows-enrollment.md §6 | doing     |
| 9   | Invite registration                  | backend done; UI missing           | Admins mint single-use expiring bearer invites, list and revoke; each redeem creates exactly one non-admin             | auth-flows-enrollment.md §6 | doing     |
| 10  | Registration/first-admin UI + toggle | missing                            | Register page (invite/open), first-admin screen via probe, panel section, `ConfigResponse.allowRegistration` read-back | auth-flows-enrollment.md §6 | todo      |
| 11  | User delete                          | no route, no UI                    | Admin-only delete (409 on last admin), removes redb record and passwd entry, confirm dialog                            | auth-flows-user-delete.md   | todo      |
| 12  | Bootstrap flag / freshness / default | done 2026-10-13                    | Closed by default; flag + passwd-file freshness gate; harness, e2e launcher and `just run` follow                      | auth-flows-enrollment.md §1 | done      |
| 13  | CLI provisioning / recovery          | missing                            | Offline `picasu-admin` add/set-password/set-admin with lockfile discipline                                             | auth-flows-enrollment.md §2 | todo      |
| 14  | Concurrent first-create race         | unverified suspicion               | Prove with a test; share the bootstrap lock only if confirmed                                                          | auth-flows-enrollment.md §1 | todo      |
| 15  | Online brute-force prevention        | missing                            | Throttling/lockout on credential endpoints (register + invites included)                                               | auth-hardening.md (F5)      | elsewhere |

Notes:

- Rows 1–4 are `done` but stay listed: any behavior change there
  re-enters review through this table.
- Row 11 is the only flow needing a new plan; its stub is
  `auth-flows-user-delete.md` (status `open`).
- Row 14's suspicion: `create_user_handler` re-checks emptiness
  without the `BOOTSTRAP_LOCK` that `bootstrap_user` takes.
