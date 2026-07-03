---
status: done
type: chore
priority: high
area: devops
---

## Fork & Rebrand: urocissa → picasu

Forked into standalone project **picasu** with renamed crate, env vars, CI, directories, and documentation. Reoriented project messaging to filesystem-first philosophy.

### Completed

1. **Rust crate rename** — `urocissa` → `picasu` in `Cargo.toml`, binary names (`picasu`, `picasu-openapi`), imports, xtask refs
2. **Environment variables** — `UROCISSA_*` → `PICASU_*` across all source, config, Docker, CI, docs
3. **Directory restructure** — `gallery-backend/` → `backend/`, `gallery-frontend/` → `frontend/`
4. **Trimming** — Removed `gallery-site/`, PowerShell scripts, NSIS installer, Windows docs, Windows CI job
5. **CI/CD** — Updated Docker image refs, GitHub Actions workflows, release artifact names
6. **Frontend** — Updated page titles, route titles, Playwright launcher, env var refs
7. **Tests** — Updated placeholder filenames, EXIF Software tags, test bootstrap structs
8. **README rewrite** — Reframed from performance-focused gallery to filesystem-first positioning. Heritage note, WIP disclaimer, simple features split (ready vs planned), condensed quick-start.
9. **Docs capitalization** — `docs/CONFIG.md` → `docs/config.md`, `docs/LINUX.md` → `docs/linux.md`, `docs/FRONTEND.md` → `docs/frontend.md`, `docs/SEARCH.md` → `docs/search.md`; updated all internal references.
10. **Branding assets** — Replaced `frontend/public/favicon.ico` with picasu favicon; removed orphan `backend/assets/logo.ico` (Windows installer remnant).
11. **Prose cleanup** — `AGENTS.md` (project name), `docs/CONFIG.md` → `docs/config.md` (urocissa → picasu references).
