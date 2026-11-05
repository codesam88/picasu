# Benchmark harness

Benchmark and debug cells for the Picasu backend. The harness is stdlib-only
Python plus the shell workload driver; it is not part of the server binary.

| Path                  | Role                                                                             |
| --------------------- | -------------------------------------------------------------------------------- |
| `backend_launcher.py` | State setup, port preflight, spawn, readiness, supervision, `launcher-info.json` |
| `launch-backend.py`   | Env-driven CLI wrapper around `backend_launcher.py`                              |
| `workload.py`         | Image-set materialisation, scans, probes and harness-A-compatible artifacts      |
| `run-workload.py`     | Env-driven CLI wrapper around `workload.py`                                      |
| `run-cell.py`         | One cell: launcher, workload, failure diagnostics, graceful shutdown             |
| `extract-bench.py`    | Summarise one or more harness output directories                                 |
| `manifests/`          | PhotoSpec manifests for fixed image sets                                         |

The environment contracts live in the module docstrings. Each producer writes
into `$OUT`; the launcher writes `launcher-info.json`, which the workload
consumes.

## Checks

```sh
just bench test    # or: just bench-test (root alias)
just bench lint    # or: just bench-lint (root alias)
```

## Canonical cell

```sh
cargo build --release --bin picasu
just bench seed-images /tmp/sets/set-100 100
just bench workload /tmp/picasu-bench-N8-100-r1 /tmp/sets/set-100 100 8
just bench extract /tmp/picasu-bench-N8-100-r1
```

`just bench workload <out> <image_repo> <images> <ncpu> [debug|release]`
runs the canonical cell (`build` and `port` take `name=value` overrides).
Binaries and the repo must exist; the stack fails with a build/seed hint
otherwise. `just bench server` serves a backend without driving any
workload.

## Image repos (seeded a priori)

Cells never generate images. Seed a repo once, reuse it for any cell size
(the workload copies the first `IMAGES` entries in sorted order):

```sh
just bench seed-images /tmp/sets/set-100 100        # 100 × 4000x3000 jpg
just bench seed-images /tmp/sets/set-500 500        # same shape, 500 images
```

Set `OVERWRITE=1` to reseed a non-empty repo.
