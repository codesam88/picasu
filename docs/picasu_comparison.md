# Picasu vs Immich - Compare & Contrast

Quick overview of Picasu goals and features as compared to Immich, a
managed-store, multi-container gallery. Both projects change rapidly; values
reflect a surveyed baseline.

## Picasu Design Goals

- Filesystem-first: if Picasu is removed, the photo set remains as directories
  of files with metadata in XMP sidecars.
- Primary use cases: frictionless mobile backup followed by lazy curation through
  the web UI. Inspired by Synology Photo Station; the backend runs on the main
  photo storage/NAS, which carries its own redundancy and off-site backup.
- Current status: there is no built-in mobile auto-backup. Mobile backup is
  handled by pointing an external sync tool (Syncthing) at the photo storage;
  Picasu indexes the synced files in place. No Picasu-side integration is
  required. A native mobile app is to be determined; the priority is a good
  mobile web app (see Planned direction).
- Robustness is a stated goal, pursued through the test strategy and API
  assurance checks (see test-strategy.md and api-assurance.md).

## Architecture

| Attribute       | Picasu                                                                                                            | Immich                                          |
| --------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| Stack           | Rust (Rocket) single binary + Vue 3 SPA                                                                           | Node (NestJS) + SvelteKit + Flutter + Python ML |
| Source of truth | Filesystem; DB is a rebuildable cache/index                                                                       | Postgres + Redis                                |
| Deploy          | Single binary with `ffmpeg` helper. Dockerfile and compose.yaml exist; Docker deployment is not covered by tests. | Docker Compose, four containers                 |
| Database        | redb index/cache (Rust native); separate user store file                                                          | Postgres                                        |
| Mobile client   | Responsive web app                                                                                                | Native mobile apps (Flutter)                    |
| License         | MIT                                                                                                               | AGPL-3.0                                        |

## Feature comparison

Values: Present / Absent / Not stated in the surveyed baseline.

| Area                                  | Picasu     | Immich     | Notes                                                                                       |
| ------------------------------------- | ---------- | ---------- | ------------------------------------------------------------------------------------------- |
| Web upload and viewing                | Present    | Present    |                                                                                             |
| Filesystem sync / import              | Present    | Present    | Picasu indexes in place; Immich offers external libraries                                   |
| Expression / filtered search          | Present    | Present    | Immich adds object/face/CLIP search (see below)                                             |
| Tags, rating, EXIF metadata           | Present    | Present    | Picasu writes metadata back as XMP sidecars                                                 |
| Duplicate detection (hash + phash)    | Present    | Present    |                                                                                             |
| Video support (ffmpeg)                | Present    | Present    |                                                                                             |
| Multi-user, admin, invites, shares    | Present    | Present    | Immich adds partner sharing and public sharing variants                                     |
| Session auth with revocation/throttle | Present    | Present    | Immich also supports OAuth                                                                  |
| Native mobile app                     | Absent     | Present    |                                                                                             |
| Mobile auto-backup                    | Absent     | Present    |                                                                                             |
| Offline / installable PWA             | Absent     | Partial    | Immich offline is mobile-only; Picasu service worker is used for share-token injection only |
| OIDC / OAuth login                    | Absent     | Present    | Picasu uses session tokens + password; OTP and passkeys planned                             |
| Granular per-asset ACLs               | Absent     | Not stated |                                                                                             |
| Account recovery flows                | Absent     | Not stated | Picasu recovery is operator-only (`--bootstrap-admin`)                                      |
| Object storage (S3/Azure)             | Absent     | Not stated | Picasu is filesystem-first                                                                  |
| Maps / geolocation view               | Absent     | Present    |                                                                                             |
| RAW image formats                     | Absent     | Present    |                                                                                             |
| LivePhoto / MotionPhoto               | Absent     | Present    |                                                                                             |
| Face recognition / clustering         | Absent     | Present    |                                                                                             |
| CLIP / semantic search                | Absent     | Present    |                                                                                             |
| User-defined storage templates        | Absent     | Present    | Picasu keeps files where the user places them                                               |
| Automated backups / migration         | Absent     | Present    | Picasu: back up filesystem + state dir; only index DB is stateful                           |
| Built-in TLS                          | Absent     | Not stated | Both typically sit behind a reverse proxy                                                   |
| 360-degree image display              | Not stated | Present    |                                                                                             |
| Memories ("x years ago")              | Not stated | Present    |                                                                                             |
| Stacked photos                        | Not stated | Present    |                                                                                             |

## Pros, cons, and decision help

**Picasu tends to fit when:**

- The filesystem is the source of truth and files are managed with external tools; the gallery should adopt changes made outside it.
- Metadata should live in interoperable XMP sidecars.
- A single binary with a small footprint and an MIT license matters.
- TLS is handled by a standard reverse proxy.
- Mobile backup runs through external sync (e.g. Syncthing) onto the photo storage, and maps and ML-based search are not required.

**Immich tends to fit when:**

- Native mobile auto-backup and offline mobile use are required.
- OIDC, maps, RAW/LivePhoto support, or face/CLIP search are required.
- User-defined storage templates and managed database backups are required.
- An AGPL-3.0 license and a four-container Postgres/Redis deployment are acceptable.

## Picasu planned direction

Product intent, without dates:

- Mobile backup/sync is handled outside Picasu with external sync tools
  (Syncthing): point sync at the photo storage and Picasu indexes the files in
  place. A native mobile app is to be determined; priority is a good mobile web
  app.
- TLS and systemd deployment are handled via a standard reverse proxy (for example nginx), not built into Picasu.
- Self-hosting comes first; hosting on AWS/Azure object storage is not a priority.
- Next auth/sharing priority: OTP, passkeys, and sharing improvements.
- AI direction: an API for external services to generate image metadata (auto-tagging, grouping, duplicate-detection, and similar), with no built-in ML service.
