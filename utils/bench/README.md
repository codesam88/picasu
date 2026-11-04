# Benchmark harness

Benchmark and debug cells for the Picasu backend. The harness is stdlib-only
Python plus the shell workload driver; it is not part of the server binary.

| Path                  | Role                                                                             |
| --------------------- | -------------------------------------------------------------------------------- |
| `backend_launcher.py` | State setup, port preflight, spawn, readiness, supervision, `launcher-info.json` |
| `launch-backend.py`   | Env-driven CLI wrapper around `backend_launcher.py`                              |
| `workload.py`         | Image-set materialisation, scans, probes and harness-A-compatible artifacts      |
| `run-workload.py`     | Env-driven CLI wrapper around `workload.py`                                      |
| `extract-bench.py`    | Summarise one or more harness output directories                                 |
| `harness-a.sh`        | Shell workload driver; also the reference protocol the wrapper reproduces        |
| `manifests/`          | PhotoSpec manifests for fixed image sets                                         |

The environment contracts live in the module docstrings. Each producer writes
into `$OUT`; the launcher writes `launcher-info.json`, which the workload
consumes.

## Checks

```sh
just bench test    # or: just bench-test (root alias)
just bench lint    # or: just bench-lint (root alias)
```
