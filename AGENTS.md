# Agent Guidelines

## Communication Style

- Stick to facts and established best practice. Do not editorialize or try to be persuasive.
- In assessments and commit messages, reflect uncertainty where it exists. Avoid asserting things as definitive when the evidence is incomplete.
- Skip superlatives and filler ("Clean.", "Perfect.", "This is the right call."). State what happened or what is true and move on.

## Project Orientation

Picasu is a self-hosted photo gallery for millions of images. The repo has two independent frontends and one backend:

| Directory   | Stack                                | Role                                                     |
| ----------- | ------------------------------------ | -------------------------------------------------------- |
| `backend/`  | Rust (Rocket, utoipa)                | HTTP API, image processing, metadata DB, file management |
| `frontend/` | Vue 3 + Vuetify + Pinia + Vue Router | Main photo gallery SPA — shipped with the server binary  |

**Entry points for a new agent:**

1. **Architecture overview** — start with `docs/frontend.md` (frontend) and `docs/design.md` (backend). They cover routing, state, component tree, filter system, and design decisions.
2. **Dev commands** — read `justfile` at the repo root. Key recipes: `just check` (lint), `just test` (backend cargo test + frontend vitest), `just frontend-check`, `just frontend-test`, `just format` (auto-format)
3. **Backend sources** — `backend/src/main.rs` (server bootstrap), `backend/src/router/` (API routes), `backend/src/operations/` (domain logic), `backend/src/process/` (background tasks).
4. **Frontend sources** — `frontend/src/main.ts` (app bootstrap), `frontend/src/route/routes.ts` (URL structure), `frontend/src/store/` (Pinia stores), `frontend/src/components/` (Vue components) — see `docs/frontend.md` for the component tree.
5. **Testing** — Review `docs/test-strategy.md` and `docs/playwright_generator.md` before extending or modifying test scenarios or test infratructure.

The `.plan/tasks/` directory tracks pending and completed work. Run `just plan -k` to view the board.

## Project Development Workflow

- Take a step back and consider best-practice solutions before diving in.
- Clarify ambiguous requirements with the user before starting.
- Always present the proposed solution and its trade-offs for review before implementing.
- Before claiming success, run applicable tests to verify. In doubt, run full test suite: `just check; just test`.
- When done, provide a summary of the change and give a chance to review or course correct. Commit only on request.
- Commit messages should include a summary of what was changed and why. Do not include verbose examples or documentation. Only large commits may contain lists of changes.
- Update code-level documentation where applicable. Refrain from including verbose examples and documentation without request.

## Task Management (.plan)

Every task lives as a markdown file in `.plan/tasks/<slug>.md` with YAML frontmatter. The filename (without `.md`) is the unique key — no numeric ID needed.

### Status lifecycle

| status        | meaning                   | when to use                                   |
| ------------- | ------------------------- | --------------------------------------------- |
| `idea`        | aspirational, not settled | explore later, not ready to start             |
| `backlog`     | accepted, deferred        | consider when stepping back to plan next work |
| `open`        | ready                     | actionable, waiting to be picked up           |
| `in-progress` | active                    | currently being worked on                     |
| `blocked`     | stuck                     | note the blocker in the body                  |
| `done`        | complete                  | finished                                      |

### Task Management Workflow

- **Discover work:** `just plan` lists all tasks; `just plan -k` groups by status column. Filter with `-s open`, `-a backend`, `-t bug`, etc.
- **Step back / plan:** `just plan -k` to see the full board. Pull items from `backlog` or `idea` when choosing what to work on next.
- **Sort:** `just plan -S priority -S slug` sorts by priority then slug. Run `plan --help` for all options.
- **Create:** copy `.plan/TEMPLATE.md` to `.plan/tasks/<slug>.md`.
- **Update:** update 'status' to reflect status. append progress notes at the bottom (newest first). Do not rewrite history.
- **Complete:** set `status: done` when finished. Do not delete the file.
- **Block:** set `status: blocked` and note the blocker in the body.
- **Validate:** run `just plan-lint` to check; `just plan-format` to auto-fix. The precommit hook runs `--format` automatically

## Code documentation

Prefer `///` doc comments in the Rust source to document function purpose and any non-obvious design decisions.
Doc comments live next to the code, get updated with it, serve `cargo doc`, IDEs, and agents equally, and never rot.

Items which do not fit into code documentation such as design decisions and usage instructions belong in separate user-focused documentation.

Only document here what cannot be derived from the code itself.
