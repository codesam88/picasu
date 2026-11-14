# Benchmark protocol

What the benchmark cells measure, why each measurement exists, and how
cells stay comparable. Harness usage (`just bench ...`) lives in
`utils/bench/README.md`; this page is the rationale behind the matrix.

## Principles

- No tuning without a measured number. A candidate change names the
  cell and metric that trigger it before it is built.
- Row-scaling and byte-scaling are measured separately. Tree rebuild,
  snapshot clone, sweep, and sort are row-count-driven and file-size
  blind; decode, thumbnail generation, and upload transfer are
  byte-driven. Conflating them wastes disk and muddies attribution.
- Cells are comparable by construction: fixed image archive, fixed
  battery env per scale, one machine-readable result row per cell.
  Drift is detected mechanically, not by eyeballing transcripts.

## Image archive and sandbox

Seeded images are small (~570 KB, 4000x3000 JPEG): 100 images are
57 MB, so 50 000 would be ~29 GB, and real-camera files (5 MB+) would
push that past 250 GB. Generating and copying that per cell is not
practical, so:

- One canonical archive per file-size class is seeded once and reused
  by every cell. The archive is read-only; cells never generate images.
- A cell materialises its set from the archive by hardlink (same
  filesystem), not by copy: materialisation is O(1) disk regardless
  of set size. Copying is reserved for filesystems where links are
  unavailable.
- The server under test points at a sandbox root (`image_home` /
  `PICASU_IMAGE_HOME`) prepared once per scale. Re-seeding or
  re-copying per run is a protocol violation unless the cell
  explicitly measures cold-start ingest.
- Two archive classes: small-file (row-scaling; counts 100 / 5650 /
  50 000+) and large-file (byte-scaling; a few hundred real-camera-
  sized files, ~2–3 GB total). The large-file class exists only for
  the decode/thumbnail/upload dimensions, never for row scaling.

## Stimulus catalog

- `control`: populate scans with during-scan random thumbnail GETs
  plus hyperfine prefetch/edit-rating probes per round, then a
  sequential sweep and post-scan baselines. Measures the request path
  under indexing load — the primary regression signal.
- `storm-read` / `storm-thumb` / `storm-diff`: concurrent bursts
  (clients × ops) of prefetches, thumbnail GETs, or edit-rating PUTs
  against a scan, with a must-finish-before-scan-end margin. Measures
  burst headroom and tail latency, not averages.
- `storm-idle`: burst against the tree at rest. Separates serving cost
  from indexing contention.
- `upload`: file ingest through the upload route. The response is
  store-and-return, so the measurement is two-sided: submit-to-200
  latency plus time-to-visible (poll until the row serves), which also
  covers background-indexing drain. Upload is the only stimulus that
  measures the write path end to end.

## Metrics catalog

Per cell, joined by scan index so request-side and server-side numbers
describe the same work:

- Request latencies: per-op medians/p95/max from storm TSVs and
  hyperfine exports (flat `mean`/`median` shape). Why: the user-facing
  regression signal; tails matter more than medians under load.
- Scan walls and convergence: per-run wall time, watermark trail,
  time-to-terminal, time-to-visible for uploads. Why: bounds the
  backstop/convergence claims with wall-clock evidence.
- Server stage costs: the observer per-scan drain table (rebuild,
  clone, sort, write-wait, permit-wait, exec stages with row-count
  context), parsed from `server.log`, which every cell keeps.
  Whether observer stats are on is configured per cell and recorded
  in the manifest. Bench mode defaults stats on (overridable to 0): a
  with/without distortion baseline at set-100, release, SCAN_REPEATS=2
  showed no measurable distortion -- stats-on scan wall 11.6s vs
  stats-off 35.1s, with per-request latencies ~7-13ms vs ~30-50%
  higher, and idle baselines within ~5% (edit the noisiest single-run
  metric at ~20%); run-to-run variance (3x scan walls back-to-back on
  the same machine) dwarfs any observer effect, consistent with the
  observer's design (seqlock/atomics, no I/O on the record path; drain
  table rebuilt once per scan). Why: attributes a
  latency change to a pipeline stage instead of a whole build.
- Failure shape: write-probe verification, margin verdicts, breach
  lines. A cell that fails verification is data about the failure,
  not a missing data point.

## Battery matrix

Profiles × scales × load states, with env pinned per battery
(`SCAN_REPEATS`, hyperfine repeats, storm clients/ops, margin):

| Profile               | 100 (gate)               | 5650 (scale)      | 50k+ (target)                |
| --------------------- | ------------------------ | ----------------- | ---------------------------- |
| control               | regression gate          | row-scaling reads | millions-image extrapolation |
| storm-read/thumb/diff | burst headroom           | burst at scale    | — unless scale gates pass    |
| storm-idle            | serving floor            | serving at scale  | —                            |
| upload                | ingest latency + visible | ingest at scale   | —                            |

The 100-image battery is the regression gate and runs on every
harness change. Larger scales run on demand and only promote upward
when the smaller scale is clean. Empty matrix cells are not run until
their prerequisite is green.

## Reporting

One machine-readable result row per cell (profile, scale, build,
commit, request latencies, scan walls, stage costs) plus the raw
artifacts. Baselines are checked in; a cell that moves a baseline
beyond its band fails the battery until the move is explained and the
baseline re-recorded. Hand-transcribed comparison tables are working
notes, not results.
