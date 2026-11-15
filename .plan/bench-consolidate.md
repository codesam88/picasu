---
status: done
type: chore
priority: high
area: backend
---

## Notes

Consolidate `utils/bench/` from accreted layers into one designed
harness: a single CLI, a single config, an explicit prepare step, a
preflight checklist, structured events instead of log dialects, and the
battery as the only runner. Greenfield analysis (2026-11-15): the
harness grew by transliteration (shell driver → Python → launcher split
→ orchestrator → battery → extractor), and each layer talks in its
predecessor's dialect — env strings, legacy log lines, scraped server
logs. Every step below removes a dialect.

Settled 2026-11-15: archaeology cleanup goes first as the behavior-neutral
clearing pass; prepare + preflight next as the enabling pair.

## Goals (keep top-of-mind)

The harness exists to produce trusted numbers for the perf-investigation
items. Characteristics it must keep through every step:

- Measurements: request latencies (per-op medians/p95/max from storm
  TSVs and hyperfine exports), scan walls and convergence
  (per-run walls, watermark trail, time-to-terminal, upload
  time-to-visible), server stage costs (observer drain tables joined by
  scan index), failure shape (write-probe verification, margin
  verdicts, breach lines). No tuning without a measured number.
- Scenarios: control (request path under indexing load), storm-read /
  thumb / diff (burst headroom), storm-idle (serving floor), upload
  (submit-to-200 plus time-to-visible, with during-scan probes so
  upload-scan interference compares against background scans).
  Profiles × scales × load states; 100-image gate on every harness
  change, larger scales only when smaller is clean; medians across
  repeats, never single cells.
- Outputs: one machine-readable row per cell plus raw artifacts, a
  pooled `:median` row per cell as the comparison unit, checked-in
  baselines with bands, `battery-check` failing out-of-band cells until
  explained and re-recorded. `battery extract` dumps a whole battery
  parent as one JSON document.

## CLI look and feel

One `bench` command; configure, launch, observe, extract:

```sh
bench setup [--sets-root DIR]          # venv, managed deps, build, seed canonical archives, verify
bench prepare <sandbox> --archive set-100 [--large]
                                       # materialise sandbox from archive, write sandbox manifest
bench preflight [--sandbox DIR]        # checklist: binary, hyperfine, archive, ports; fail before spawning
bench run <battery|cell> --out <parent> [--fresh]
                                       # the only runner; single cell = one-cell battery
bench status <parent>                  # per-cell state: queued/running/finished, rc, wall so far
bench extract <parent> [--out FILE]    # rows + stages + manifests as one JSON document
bench battery-check <report>           # conformance against baselines.tsv
bench seed-images ...                  # kept as the archive-seeding primitive
```

`status` doubles as the debug entry: points at the failing cell's
`server.log` tail and `diag-orchestrator/` bundle instead of grepping
outdirs by hand. Today's four entry points (`launch-backend.py`,
`run-workload.py`, `run-cell.py`, `battery.py run`) and the justfile
`workload` recipe collapse into `bench run`; the recipe's hidden
`timeout` wrapper goes away in favor of the per-level budget.

## Steps

Each step is one worker invocation: RED observed, implementation,
GREEN, review, commit. No step changes measured behavior except where
noted; the gate battery stays green throughout (spot-check `bench
battery gate` after steps 2-5).

1. [architecture-neutral cleanup] Delete the archaeology: `compressed-get`
   wording branches, `harness-A` naming, `N`→`NCPU` guard, `DATASET_ID`
   aliases, `__getattr__` shims, stale docstrings. Tests pin current
   behavior first where missing; extractor keeps parsing all recorded
   cells byte-identically.
2. [`bench prepare`] One explicit step builds a sandbox from a canonical
   archive and writes a sandbox manifest (archive id, count, bytes,
   linkage). Cells point at prepared sandboxes and never materialise;
   link-or-copy fallback, `MATERIALISE`, and the manifest
   read-modify-write go away. `IMAGE_HOME_SRC` becomes "prepared by
   `bench prepare`".
3. [`bench preflight`] One checklist phase (binary, hyperfine, archive,
   ports, sandbox manifest) that fails before any process spawns, with
   one message shape. Absorbs the scattered guards; a cell whose
   preconditions don't hold is refused, never failed 20 minutes in.
4. [one CLI, one config] Single entry point with subcommands and one
   validated config object built once, replacing per-module
   `config_from_env` parsing. Justfile recipes become thin aliases.
5. [structured events] Workload emits JSONL events; observer data
   arrives as data (endpoint snapshot or harness-curled file), not
   scraped logs. Extractor becomes a pure function over typed rows;
   regex generations collapse to a versioned schema reader.
6. [battery absorbs run-cell] The battery is the only runner; the
   justfile `workload` recipe becomes a one-cell level. Resume,
   repeats, medians, and report logic live in exactly one place.
7. [golden-file rule] Parser fixtures are recorded real artifacts, not
   fabricated lines; the S2 miss (green tests, empty `stages.json` in
   real cells) is the standing example in the test docs.
8. [enforced parents] The runner owns outdir creation under its parent
   and refuses foreign outdirs; manual loose cells become impossible.

Order: 1, then 2+3 (either order), then 4, then 5+6, then 7+8.

## Completion (2026-11-15)

Steps 1-8 implemented on `bench/work` (commits `26f3dfba`..`7b6ccbcf`;
suite 139 → 256 tests, all green; gate baselines recorded separately).
Deliberately left out, future work, not oversights:

- `bench setup` from the CLI sketch (venv + managed deps + build +
  seed + verify in one command) was never a numbered step and is not
  built. The pieces all exist (`just bench` recipes, `install`,
  `seed-images`, `preflight`).
- Observer-as-data (endpoint snapshot or harness-curled file instead of
  scraped `server.log`) needs a backend change; explicitly out of bench
  scope. The drain-table parse stays.
- Transcript-sourced leftovers noted at 5b (identity, post-scan
  hyperfine, upload p50s, `scan state=` keys) stay until a deliberate
  re-record; precision pinning keeps current baselines comparable.
