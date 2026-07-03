---
status: backlog
type: chore
priority: low
area: devops
---

Docker deployment items:

1. **Verify the rework actually works** — `docker compose build`, confirm the image builds and `embed-frontend` actually picks up the frontend dist copied into the builder stage; `docker compose up`, confirm config/data/images land in the bind-mounted host dirs and the app is reachable; confirm `imagePath` set to `.` resolves against `/images` inside the container.

2. **Publish to `ghcr.io/codesam/urocissa`** (GitHub Container Registry) — free for public images, co-located with source; complement or replace Docker Hub `hsa00000/urocissa`, which this fork can't push to.

3. **Add a systemd `.service` file** in `deploy/` or `contrib/` for users running the binary directly (non-Docker), using `UROCISSA_CONFIG_HOME`/`UROCISSA_DATA_HOME`/`UROCISSA_IMAGE_HOME` env vars.

4. **`UROCISSA_STATE_HOME`** — split disposable cache files from irreplaceable data. `UROCISSA_DATA_HOME` holds both `db/index_v5.redb` (irreplaceable) and cache files (`db/cache_db.redb`, `db/temp_db.redb`, `db/expire_db.redb`). Split into a proper state directory. Touches each redb file's path individually (`tree_snapshot/new.rs`, `query_snapshot/new.rs`, `expire/new.rs`). Not done yet.
