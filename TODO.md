# Local backlog (gitignored)

- standalone TLS deployment path: add `tls` Cargo feature gating `rocket/tls`, validate end-to-end with a real cert (certbot/acme.sh), document in CONFIG.md; only ship once tested
- review for license issues, SPDX label and OSSF score card
- review configuration options against docs
- add a proper documentation / generation system
- move the fork to github, compatible CI flows and Android app by Ted(?)
- enable the release/deployment flows in CI
- add android app 
- review code module by module

## Github fork

- [ ] work on github fork for more compatible CI + contributing back
- [x] fork main
- [ ] push all changes to a dev branch
- [ ] refactor changes to first apply devops, tests, audit fixes, then merge / compress the album stuff?

## Docker / Deployment (assessed 2026-06-15)

The current setup has two pieces: `docker/Dockerfile` for building and `run_urocissa_docker.sh` for running the upstream-published image (`hsa00000/urocissa:latest`).

### Issues with current approach

- **Dockerfile in `docker/`** — non-standard; most tooling (GitHub Actions `docker/build-push-action`, Docker Hub auto-builds, `docker build .`) expects it at repo root. Requires `-f docker/Dockerfile` everywhere.
- **Run shell script (~230 lines)** — does what Docker Compose handles in ~20 lines: volume mounts, port mapping, env vars. Raw `sed`/`grep` JSON parsing of `syncPaths` and port is fragile.
- **Entrypoint `mv` pattern** — moves binaries from image into `UROCISSA_PATH` at runtime; non-standard and fragile. Normal pattern: mount host dirs into fixed image paths.

### Recommended improvements

- [ ] Add `docker-compose.yml` at repo root — replaces the shell script; `docker compose up -d` is the standard UX users expect. Highest value single improvement.
- [ ] Move `docker/Dockerfile` to repo root; keep `docker/` for compose, `.dockerignore`, and CI helpers only.
- [ ] Add a systemd `.service` file in `deploy/` or `contrib/` for users running the binary directly (non-Docker).
- [ ] Publish to `ghcr.io/codesam/urocissa` (GitHub Container Registry) — free for public images, co-located with source; complement or replace Docker Hub `hsa00000/urocissa`.

---

## Code conventions and tooling notes

### Pre-commit hook order
`just precommit` runs in this order: `cargo fmt --check` → `cargo clippy` → `cargo nextest` → `prettier --check` → `vue-tsc + eslint` → `vitest`. Run `cargo fmt` and `npx prettier --write` before staging to avoid the most common late failures. ESLint and vue-tsc run after prettier, so formatting errors mask type/lint errors until fixed.

### Error code policy (backend)
`ErrorKind::NotFound` maps to HTTP 404, which Rocket also returns for unregistered routes. Using `NotFound` for domain-level "entity not found" makes those errors indistinguishable from routing failures in tests and clients. Use `ErrorKind::InvalidInput` (→ 400) when the route succeeded but a referenced entity does not exist. Only use `NotFound` when a routing-level 404 is genuinely the right signal.

### ESLint strict mode (frontend)
`@typescript-eslint/strict-boolean-expressions` and `@typescript-eslint/no-non-null-assertion` are enforced. These only surface at commit time (pre-commit hook). When writing TypeScript: use `=== null` / `=== undefined` instead of truthiness checks on nullable strings; avoid `!` non-null assertions — use an explicit null/undefined guard branch instead.

---

## Album feature — open items

- [ ] **E2E test for `create_dir_album`** — endpoint has no scenario coverage. Should verify: subdir is created on disk, returns new album ID, parent album updates.
- [ ] **Stale `DIR_ALBUM_CACHE` scenario** — if a directory is deleted externally while the cache still holds its entry, `assign_album` will attempt to move a file into a non-existent path. Decide: detect and evict stale entries at startup, or return a clear 400 with a meaningful message at request time.
- [ ] **`assign_album` file-move verification in tests** — Scenario H checks album membership after assignment but does not verify the file moved to the correct directory on disk. Add a filesystem assertion to close this gap.

---

## Testing — best value items

### Backend unit tests (low effort, no DB needed)
- [x] `prettify_dir_name` — pure string transform, dir_album.rs
- [x] schema version round-trips — encode v1/v2, decode, check fields; ser_de.rs
- [x] `Expression` filter predicates — construct filter, run against fixture AbstractData; generate_filter.rs
- [x] `compute_timestamp` priority logic — abstract_data.rs
- [x] `belongs_to_album` path-prefix logic — combined.rs (dir vs manual branching)

### Backend integration tests (need tempdir redb — not as hard as it sounds)
- [ ] index → dedup → flush → album self-update round-trip
- [ ] dir album path-prefix membership (create album, check file in/out of subtree)
- [x] schema migration: write v1-encoded record, read back as v2 (through redb)

### Backend tooling (trivial)
- [x] deny(unsafe_code) — enforced at compile time via main.rs
- [x] cargo nextest — installed, replacing cargo test in justfile
- [x] cargo audit — in justfile (`just backend-audit`); known-unfixable advisories suppressed in `.cargo/audit.toml` with enforcement tests
- [x] cargo deny — deny.toml in place; wired into just audit
- [x] tighten clippy: unwrap_used = warn in Cargo.toml; excluded from -D warnings until call sites are cleaned up
- [ ] cargo geiger — include in release/audit reports (unsafe surface area of dep tree)
- [ ] semi-regular security audit CI job — run `just audit` on a schedule (monthly or on dep changes); options: Codeberg CI (Woodpecker), GitHub Actions on the upstream fork, or a local cron on the dev machine; the `/security-audit` skill covers the manual review workflow when issues are found

### Frontend unit tests
- [x] Vitest — installed; 34 lexer tests covering all atom types, compound operators, escaping, and lex errors
- [ ] Pinia store reducers — pure logic in stores, no DOM needed

### Frontend tooling
- [ ] knip — periodic dead-export sweep, not in precommit
- [x] npm audit — in justfile (`just frontend-audit`); all vulnerabilities resolved by bumping to latest deps
- [ ] dependency updates — establish a regular cadence (npm-check-updates, Dependabot, or Renovate); small frequent updates are cheaper than batched drift
- [ ] Zod (or valibot) — investigate for runtime validation of API responses; TypeScript types don't catch backend schema drift at runtime

### Deferred
- [ ] Playwright E2E — needs running backend; save for CI
- [ ] cargo bench / criterion — indexing pipeline regression benchmarks (file hashing, EXIF extraction, thumbnail generation, redb write throughput, serving latency under load)
- [ ] Admin status API — expose queue depth, active indexing jobs, recent errors, per-job progress as a JSON endpoint (foundation for a future admin panel)
- [ ] Frontend progress UI — poll the existing `/get/import/folder/status` pattern (or a future SSE stream) to show active indexing progress to authenticated users
- [ ] Structured logging — investigate `tracing` + `tracing-subscriber` as a replacement for `env_logger` to support spans, JSON output for log aggregators, and `tracing-journald` for native systemd/journald integration
