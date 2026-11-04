#!/usr/bin/env bash
# Harness A: baseline measurement for the worker-pool rework.
#
# Generates a jpg-only snapfab image set, boots the server under a fixed CPU
# budget, triggers an album index scan, polls it to a terminal state, and
# measures fetch latency via per-round hyperfine runs during the scan plus a
# final sequential curl sweep and a post-scan hyperfine run. A third
# hyperfine command in every round probes the write path: PUT /put/edit_rating
# sets rating 3 on snapshot index 0. The value is constant so repeats are
# idempotent (no state toggling across rounds); the only mutation is a
# benign .xmp sidecar write under the scratch $OUT/images. Once post-scan,
# the applied value is verified through GET /get/metadata (list rows are
# lean and carry no rating, so get-data cannot prove the write applied).
#
# Usage:
#   utils/bench/harness-a.sh
#
# Environment:
#   N            cores for the server's taskset budget (default 4)
#   COUNT        image count when generating a manifest (default 10)
#   REPEATS      post-scan hyperfine runs (default 3)
#   WARMUP       hyperfine warmup runs (default 1)
#   BUILD_PROFILE cargo profile for picasu+snapfab: debug (default, fast
#                build) or release (production-representative numbers;
#                keep constant across cells being compared)
#   RUNS_DURING  per-round hyperfine runs during the scan (default 3;
#                per-round warmup is always 0 to keep rounds cheap)
#   FETCHES      random compressed fetches per during-scan round, hit/miss
#                reported separately (default 10)
#   CONCURRENCY  retained for log compat; the concurrent curl sweep no longer
#                exists (default: N)
#   MANIFEST     PhotoSpec manifest to use instead of generating one (optional)
#   GEN_JOBS     parallel `snapfab batch` workers for image generation
#                (default: min(4, nproc); GEN_JOBS=1 keeps the sequential path).
#                Capped at 4 because each snapfab worker fans out internally
#                (rayon pixel loops) and per-image cost varies by generator
#                algo, so more workers oversubscribe and add noise. Manifest
#                entries are dealt round-robin across workers (entry i goes to
#                worker i%K) so each worker gets a generator-algo mix instead
#                of one contiguous slice.
#   REUSE_IMAGES pre-generated image directory to copy into $OUT/images
#                instead of running snapfab (read-only use of the source;
#                filenames are verified against the manifest). Works with
#                either MANIFEST or a generated COUNT manifest when the
#                reused set uses the same bench_%04d.jpg naming.
#   SKIP_GENERATE=1
#                skip generation entirely; images are expected to already be
#                in $OUT/images. Requires MANIFEST (the set's own manifest).
#   OUT          output directory (default /tmp/opencode/worker-pool-rework/harness-sanity)
#   PORT         server port (default 5713)
#   PROBE_LIMIT  max compressed files probed (default: COUNT)
#   SCAN_TIMEOUT_S  scan poll budget in seconds (default 0 = wait
#                   indefinitely until a terminal state; the harness must
#                   not exit or start post-scan work while state=running)
#   SCAN_REPEATS   index-scan repetitions per cell (default 3); the median
#                   wall time is reported as scan_time
#
# Canonical reusable image sets (jpg-only 4000x3000 + exif_date + tags, same
# PhotoSpec shape as utils/bench/manifests/scan-10.json) live under
# /tmp/opencode/worker-pool-rework/sets/ as <name>/ + <name>.json, with a
# per-file sha256 list (<name>-files.sha256), a manifest sha256 (<name>.sha256)
# and a count/bytes record (<name>.meta). Reuse invocations:
#   MANIFEST=/tmp/opencode/worker-pool-rework/sets/set-100.json \
#   REUSE_IMAGES=/tmp/opencode/worker-pool-rework/sets/set-100 \
#   OUT=<out> N=<n> utils/bench/harness-a.sh
# or (caller pre-populates $OUT/images, e.g. a bind of the set dir):
#   MANIFEST=/tmp/opencode/worker-pool-rework/sets/set-100.json \
#   SKIP_GENERATE=1 OUT=<out> N=<n> utils/bench/harness-a.sh
#
# All artifacts land in $OUT: server log, status snapshots, per-round
# hyperfine JSON, the final sweep TSV, post-scan hyperfine JSON, and
# harness-a.log (the run transcript).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

N="${N:-4}"
COUNT="${COUNT:-10}"
REPEATS="${REPEATS:-3}"
WARMUP="${WARMUP:-1}"
RUNS_DURING="${RUNS_DURING:-3}"
FETCHES="${FETCHES:-10}"
CONCURRENCY="${CONCURRENCY:-$N}"
PORT="${PORT:-5713}"
OUT="${OUT:-/tmp/opencode/worker-pool-rework/harness-sanity}"
MANIFEST="${MANIFEST:-}"
PROBE_LIMIT="${PROBE_LIMIT:-$COUNT}"
SCAN_TIMEOUT_S="${SCAN_TIMEOUT_S:-0}"
ADMIN_ID="${ADMIN_ID:-bench-admin}"

# Throwaway per-run credential: generated, never stored, only valid against the
# wiped data dir this run creates (PICASU_TRIVIAL_PASSWORDS is on).
printf -v ADMIN_PW 'bench-%04x-%04x' "$$" "$RANDOM"

LOG="$OUT/harness-a.log"
BASE="http://127.0.0.1:$PORT"
COOKIES="$OUT/cookies.txt"
SERVER_PID=""
PROBE_PID=""

log() {
  printf '%s %s\n' "$(date +%H:%M:%S)" "$*" | tee -a "$LOG"
}

die() {
  log "ERROR: $*"
  exit 1
}

now_ms() {
  date +%s%3N
}

# --- output directory -------------------------------------------------------
case "$OUT" in
  "" | /) die "refusing to use OUT='$OUT'" ;;
  "$REPO_ROOT" | "$REPO_ROOT"/*) die "OUT must not be inside the repo: $OUT" ;;
esac
rm -rf "$OUT"
mkdir -p "$OUT"
: >"$LOG"

cleanup() {
  if [ -n "$PROBE_PID" ] && kill -0 "$PROBE_PID" 2>/dev/null; then
    kill "$PROBE_PID" 2>/dev/null || true
  fi
  if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill -TERM "$SERVER_PID" 2>/dev/null || true
    for _ in $(seq 1 50); do
      kill -0 "$SERVER_PID" 2>/dev/null || break
      sleep 0.2
    done
    kill -KILL "$SERVER_PID" 2>/dev/null || true
    log "server stopped (pid=$SERVER_PID)"
  fi
}
trap cleanup EXIT

# --- preflight -------------------------------------------------------------
log "=== harness A preflight ==="
log "repo=$REPO_ROOT"
for tool in cargo curl jq taskset xargs awk date exiftool; do
  if command -v "$tool" >/dev/null 2>&1; then
    log "tool $tool: $(command -v "$tool")"
  else
    log "tool $tool: MISSING"
  fi
done
if command -v cpulimit >/dev/null 2>&1; then
  log "tool cpulimit: $(command -v cpulimit)"
else
  log "tool cpulimit: MISSING (not installed; taskset carries the core budget)"
fi
if command -v hyperfine >/dev/null 2>&1; then
  HYPERFINE_BIN="$(command -v hyperfine)"
elif [ -x /tmp/opencode/worker-pool-rework/bin/hyperfine ]; then
  HYPERFINE_BIN=/tmp/opencode/worker-pool-rework/bin/hyperfine
else
  HYPERFINE_BIN=""
fi
if [ -n "$HYPERFINE_BIN" ]; then
  log "tool hyperfine: $HYPERFINE_BIN ($("$HYPERFINE_BIN" --version))"
else
  log "tool hyperfine: MISSING (repeat loop fallback will be used)"
fi
log "nproc=$(nproc) taskset_affinity_nproc=$(taskset -c "0-$((N - 1))" nproc)"

# --- binaries --------------------------------------------------------------
# BUILD_PROFILE=debug (default, fast to build) or release (slow to build,
# production-representative numbers). All comparisons must use the same
# profile; record it via the run-parameters log line below.
BUILD_PROFILE="${BUILD_PROFILE:-debug}"
case "$BUILD_PROFILE" in
  debug) CARGO_PROFILE_ARGS=""; BUILD_DIR="debug" ;;
  release) CARGO_PROFILE_ARGS="--release"; BUILD_DIR="release" ;;
  *) die "BUILD_PROFILE must be debug or release" ;;
esac
log "=== building binaries (profile=$BUILD_PROFILE) ==="
( cd "$REPO_ROOT" && cargo build $CARGO_PROFILE_ARGS --bin picasu ) >"$OUT/build-picasu.log" 2>&1 \
  || { tail -20 "$OUT/build-picasu.log" | tee -a "$LOG"; die "cargo build --bin picasu failed"; }
( cd "$REPO_ROOT" && cargo build $CARGO_PROFILE_ARGS -p snapfab ) >"$OUT/build-snapfab.log" 2>&1 \
  || { tail -20 "$OUT/build-snapfab.log" | tee -a "$LOG"; die "cargo build -p snapfab failed"; }
PICASU_BIN="$REPO_ROOT/target/$BUILD_DIR/picasu"
SNAPFAB_BIN="$REPO_ROOT/target/$BUILD_DIR/snapfab"
[ -x "$PICASU_BIN" ] || die "missing $PICASU_BIN"
[ -x "$SNAPFAB_BIN" ] || die "missing $SNAPFAB_BIN"
log "picasu=$PICASU_BIN snapfab=$SNAPFAB_BIN"

# --- image set -------------------------------------------------------------
IMG_DIR="$OUT/images"
mkdir -p "$IMG_DIR"
# Cap parallel generation workers: each snapfab worker fans out internally
# via rayon pixel loops and per-image cost varies by generator algo, so
# stacking many workers oversubscribes the machine and adds noise.
_DEFAULT_JOBS="$(nproc)"
if [ "$_DEFAULT_JOBS" -gt 4 ]; then
  _DEFAULT_JOBS=4
fi
GEN_JOBS="${GEN_JOBS:-$_DEFAULT_JOBS}"
REUSE_IMAGES="${REUSE_IMAGES:-}"
SKIP_GENERATE="${SKIP_GENERATE:-0}"
case "$GEN_JOBS" in
  '' | *[!0-9]* | 0) GEN_JOBS=1 ;;
esac
if [ "$SKIP_GENERATE" = "1" ] && [ -z "$MANIFEST" ]; then
  die "SKIP_GENERATE=1 requires MANIFEST=<set manifest>"
fi
if [ -n "$MANIFEST" ]; then
  [ -f "$MANIFEST" ] || die "manifest not found: $MANIFEST"
  MANIFEST_PATH="$MANIFEST"
  log "manifest=$MANIFEST_PATH (external)"
else
  MANIFEST_PATH="$OUT/manifest.json"
  {
    printf '['
    for ((i = 1; i <= COUNT; i++)); do
      day=$(( (i % 28) + 1 ))
      hour=$(( (i - 1) % 24 ))
      if [ "$i" -gt 1 ]; then printf ','; fi
      printf '{"output":"bench_%04d.jpg","format":"jpeg","width":4000,"height":3000,"exif_date":"2024:06:%02d %02d:15:00","tags":["bench","harness-a"]}' \
        "$i" "$day" "$hour"
    done
    printf ']\n'
  } >"$MANIFEST_PATH"
  log "manifest=$MANIFEST_PATH (generated for COUNT=$COUNT)"
fi
EXPECTED_ENTRIES="$(jq length "$MANIFEST_PATH")"
log "manifest entries=$EXPECTED_ENTRIES format=jpeg width=4000 height=3000 minimal=absent"

log "=== generating images (gen_jobs=$GEN_JOBS reuse=${REUSE_IMAGES:-none} skip_generate=$SKIP_GENERATE) ==="
gen_start_ms="$(now_ms)"
GEN_MODE="sequential"
if [ -n "$REUSE_IMAGES" ]; then
  # Reuse mode: copy a pre-generated canonical set read-only into IMG_DIR.
  [ -d "$REUSE_IMAGES" ] || die "REUSE_IMAGES not a directory: $REUSE_IMAGES"
  GEN_MODE="reuse($REUSE_IMAGES)"
  cp -p "$REUSE_IMAGES"/*.jpg "$IMG_DIR"/ \
    || die "copying images from $REUSE_IMAGES failed"
  log "reused images copied from $REUSE_IMAGES (source untouched)"
elif [ "$SKIP_GENERATE" = "1" ]; then
  # Skip mode: the caller placed the set's images in IMG_DIR already.
  GEN_MODE="skipped"
  log "SKIP_GENERATE=1: expecting images already in $IMG_DIR"
elif [ "$GEN_JOBS" -gt 1 ] && [ "$EXPECTED_ENTRIES" -gt 1 ]; then
  # Parallel mode: deal manifest entries round-robin across workers (entry i
  # goes to worker i%K) so each worker gets a mix of generator algos instead
  # of one contiguous slice. Each chunk holds entries with distinct "output"
  # filenames, so workers never write the same file.
  SHARDS="$GEN_JOBS"
  if [ "$SHARDS" -gt "$EXPECTED_ENTRIES" ]; then
    SHARDS="$EXPECTED_ENTRIES"
  fi
  GEN_MODE="parallel(jobs=$SHARDS,deal=round-robin)"
  CHUNK_DIR="$OUT/manifest-chunks"
  mkdir -p "$CHUNK_DIR"
  PIDS=""
  for ((k = 0; k < SHARDS; k++)); do
    jq --argjson k "$k" --argjson n "$SHARDS" \
      '[. as $in | range(0; ($in | length)) | select(. % $n == $k) | $in[.]]' \
      "$MANIFEST_PATH" >"$CHUNK_DIR/chunk-$k.json"
    ( cd "$IMG_DIR" && "$SNAPFAB_BIN" batch "$CHUNK_DIR/chunk-$k.json" ) \
      >"$OUT/gen-chunk-$k.log" 2>&1 &
    PIDS="$PIDS $!"
  done
  GEN_FAILED=0
  for pid in $PIDS; do
    if ! wait "$pid"; then
      GEN_FAILED=1
    fi
  done
  cat "$OUT"/gen-chunk-*.log >>"$LOG"
  if [ "$GEN_FAILED" -ne 0 ]; then
    die "parallel generation failed (see $OUT/gen-chunk-*.log)"
  fi
else
  ( cd "$IMG_DIR" && "$SNAPFAB_BIN" --verbose batch "$MANIFEST_PATH" ) 2>&1 | tee -a "$LOG"
fi
gen_end_ms="$(now_ms)"
# Every manifest entry must resolve to a file: count check plus per-name
# check (also catches chunk overlap/gap bugs in parallel mode).
MISSING=0
while IFS= read -r name; do
  if [ ! -f "$IMG_DIR/$name" ]; then
    log "missing image: $name"
    MISSING=$((MISSING + 1))
  fi
done < <(jq -r '.[].output' "$MANIFEST_PATH")
if [ "$MISSING" -ne 0 ]; then
  die "missing $MISSING manifest images in $IMG_DIR"
fi
IMG_COUNT="$(find "$IMG_DIR" -maxdepth 1 -type f -name '*.jpg' | wc -l | tr -d ' ')"
IMG_BYTES="$(find "$IMG_DIR" -maxdepth 1 -type f -name '*.jpg' -printf '%s\n' | awk '{s += $1} END {print s + 0}')"
[ "$IMG_COUNT" -eq "$EXPECTED_ENTRIES" ] || die "expected $EXPECTED_ENTRIES images, found $IMG_COUNT"
log "generated=$IMG_COUNT bytes=$IMG_BYTES wall_ms=$((gen_end_ms - gen_start_ms)) mode=$GEN_MODE"

# --- run parameters (echoed into the log) ----------------------------------
log "=== run parameters ==="
log "N=$N taskset_cores=0-$((N - 1)) nproc_under_affinity=$(taskset -c "0-$((N - 1))" nproc)"
log "image_count=$IMG_COUNT image_bytes=$IMG_BYTES"
log "pool_sizes: CURRENT_NUM_THREADS=$N -> single tokio RUNTIME worker_threads=$N; global rayon pool max(1,N-1)"
log "hyperfine=$HYPERFINE_BIN warmup=$WARMUP repeats=$REPEATS runs_during=$RUNS_DURING scan_repeats=$SCAN_REPEATS build_profile=$BUILD_PROFILE concurrency=$CONCURRENCY port=$PORT"

# --- server ----------------------------------------------------------------
log "=== starting server ==="
DATA_HOME="$OUT/data"
CONFIG_HOME="$OUT/config"
mkdir -p "$DATA_HOME" "$CONFIG_HOME"
env PICASU_PORT="$PORT" \
  PICASU_DATA_HOME="$DATA_HOME" \
  PICASU_CONFIG_HOME="$CONFIG_HOME" \
  PICASU_IMAGE_HOME="$IMG_DIR" \
  PICASU_USERS_HOME="$DATA_HOME/users" \
  PICASU_BOOTSTRAP_ADMIN=true \
  PICASU_TRIVIAL_PASSWORDS=true \
  PICASU_WEAK_PWHASH=1 \
  taskset -c "0-$((N - 1))" "$PICASU_BIN" >"$OUT/server.log" 2>&1 &
SERVER_PID=$!

deadline=$((SECONDS + 120))
until curl -sf -o /dev/null "$BASE/get/server-info"; do
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    tail -30 "$OUT/server.log" | tee -a "$LOG"
    die "server exited during startup"
  fi
  [ "$SECONDS" -lt "$deadline" ] || { tail -30 "$OUT/server.log" | tee -a "$LOG"; die "server not ready within 120s"; }
  sleep 0.2
done
SERVER_THREADS="$(ls "/proc/$SERVER_PID/task" | wc -l | tr -d ' ')"
log "server ready pid=$SERVER_PID threads_observed=$SERVER_THREADS server_log=$OUT/server.log"

# --- auth ------------------------------------------------------------------
log "=== enrolling + login ==="
code="$(curl -s -o "$OUT/bootstrap.json" -w '%{http_code}' -X POST -H 'Content-Type: application/json' \
  -d "{\"userId\":\"$ADMIN_ID\",\"password\":\"$ADMIN_PW\"}" "$BASE/post/users/bootstrap")"
[ "$code" = "200" ] || { cat "$OUT/bootstrap.json" | tee -a "$LOG"; die "bootstrap returned $code"; }
log "bootstrap=200 $(jq -c . "$OUT/bootstrap.json")"

code="$(curl -s -c "$COOKIES" -o "$OUT/login.json" -w '%{http_code}' -X POST -H 'Content-Type: application/json' \
  -d "{\"userId\":\"$ADMIN_ID\",\"password\":\"$ADMIN_PW\"}" "$BASE/post/users/authenticate")"
[ "$code" = "200" ] || { cat "$OUT/login.json" | tee -a "$LOG"; die "login returned $code"; }
grep -q 'jwt' "$COOKIES" || die "login set no jwt cookie"
log "login=200 cookie_jar=$COOKIES"

# --- probe plumbing (prefetch -> get-data rows) ----------------------------
build_probe_list() {
  local code ts tok
  code="$(curl -s -b "$COOKIES" -o "$OUT/prefetch.json" -w '%{http_code}' -X POST \
    -H 'Content-Type: application/json' -d '{}' "$BASE/get/prefetch")" || return 1
  [ "$code" = "200" ] || return 1
  ts="$(jq -r '.prefetch.timestamp // empty' "$OUT/prefetch.json")"
  tok="$(jq -r '.token // empty' "$OUT/prefetch.json")"
  [ -n "$ts" ] && [ -n "$tok" ] || return 1
  code="$(curl -s -b "$COOKIES" -H "Authorization: Bearer $tok" -o "$OUT/get-data.json" -w '%{http_code}' \
    "$BASE/get/get-data?timestamp=$ts&start=0&end=1000")" || return 1
  [ "$code" = "200" ] || return 1
  jq -r --argjson limit "$PROBE_LIMIT" \
    '[.[] | select(.abstractData.type == "image")][0:$limit][] | [.abstractData.id, .token] | @tsv' \
    "$OUT/get-data.json" >"$OUT/probe-list.tsv" || return 1
  [ -s "$OUT/probe-list.tsv" ]
}

run_sequential_probes() {
  : >"$OUT/probe-seq.tsv"
  while IFS=$'\t' read -r hash token; do
    [ -n "$hash" ] || continue
    local res
    res="$(curl -s -o /dev/null -b "$COOKIES" -H "Authorization: Bearer $token" \
      -w '%{http_code}\t%{time_total}' \
      "$BASE/object/compressed/${hash:0:2}/$hash.jpg")" || res=$'curl_error\t0'
    printf '%s\t%s\n' "$hash" "$res" >>"$OUT/probe-seq.tsv"
  done <"$OUT/probe-list.tsv"
}

probe_stats() {
  awk -F'\t' -v label="$2" '
    { n++; t = $3 + 0; sum += t; if (n == 1 || t < min) min = t; if (t > max) max = t; if ($2 == "200") ok++ }
    END {
      if (n > 0) printf "%s: n=%d ok=%d mean=%.6f min=%.6f max=%.6f\n", label, n, ok + 0, sum / n, min, max
      else printf "%s: EMPTY\n", label
    }' "$1"
}

# Random compressed-fetch sampling: row order shifts as the scan grows,
# so each round fetches FETCHES random rows and reports the mean (cache
# hits and freshly generated files blend in — the randomness is the
# point).
random_fetch_round() {
  local round="$1" rows="$2" state="$3"
  local sample="$ROUND_DIR/probe-round-$round.tsv"
  local h t code secs
  : >"$sample"
  shuf -n "$FETCHES" --random-source=/dev/urandom "$OUT/probe-list.tsv" 2>/dev/null \
    | while read -r h t; do
      [ -n "$h" ] || continue
      out="$(curl -s -o /dev/null -b "$COOKIES" -H "Authorization: Bearer $t" \
        -w '%{http_code}\t%{time_total}' "$BASE/object/compressed/${h:0:2}/$h.jpg")" || out="000	0"
      printf '%s\t%s\n' "$h" "$out" >>"$sample"
    done
  thumb="$(awk -F'\t' '{n++; s+=$3} END {if (n) printf "n=%d mean=%.4f", n, s/n; else print "n=0"}' "$sample")"
  log "probe round=$round rows=$rows state=$state fetch: thumb: $thumb"
}

# One probe round over the rows visible now: FETCHES random compressed
# fetches plus hyperfine prefetch and the edit-rating write probe
# (--warmup 0 -r $RUNS_DURING), JSON to hyperfine-round-<n>.json.
# No full-list sweeps run during the scan.
probe_round() {
  local round="$1" rows="$2" state="$3"
  local json="$ROUND_DIR/hyperfine-round-$round.json"
  local ts summary
  # Same snapshot the row list was read from: edit_rating addresses assets
  # by snapshot position, so the write probe uses this round's ts.
  ts="$(jq -r '.prefetch.timestamp // empty' "$OUT/prefetch.json")"
  if [ -z "$ts" ]; then
    log "probe round=$round rows=$rows state=$state no prefetch timestamp; skipping round"
    return 1
  fi
  random_fetch_round "$round" "$rows" "$state"
  if [ -n "$HYPERFINE_BIN" ]; then
    "$HYPERFINE_BIN" --warmup 0 -r "$RUNS_DURING" --export-json "$json" \
      -n "prefetch" \
      "curl -s -o /dev/null -b $COOKIES -X POST -H 'Content-Type: application/json' -d '{}' -w '%{http_code} %{time_total}\n' $BASE/get/prefetch" \
      -n "edit-rating" \
      "curl -s -o /dev/null -b $COOKIES -X PUT -H 'Content-Type: application/json' -d '{\"indexArray\":[0],\"timestamp\":$ts,\"rating\":3}' -w '%{http_code} %{time_total}\n' $BASE/put/edit_rating" \
      >"$ROUND_DIR/hyperfine-round-$round.out" 2>&1 || {
        log "probe round=$round rows=$rows state=$state hyperfine FAILED (see $ROUND_DIR/hyperfine-round-$round.out)"
        return 1
      }
    jq -e '.results | length == 2' "$json" >/dev/null || {
      log "probe round=$round rows=$rows state=$state hyperfine json has no results"
      return 1
    }
    summary="$(jq -r '.results[] | "\(.name) mean=\(.summary.time_wall_clock.mean) s median=\(.summary.time_wall_clock.median) s runs=\(.summary.time_wall_clock.count)"' "$json" | paste -sd' | ' -)"
  else
    local t2 t3
    t2="$(curl -s -o /dev/null -b "$COOKIES" -X POST -H 'Content-Type: application/json' -d '{}' \
      -w '%{time_total}' "$BASE/get/prefetch")" || t2="curl_error"
    t3="$(curl -s -o /dev/null -b "$COOKIES" -X PUT -H 'Content-Type: application/json' \
      -d "{\"indexArray\":[0],\"timestamp\":$ts,\"rating\":3}" \
      -w '%{time_total}' "$BASE/put/edit_rating")" || t3="curl_error"
    summary="prefetch single=$t2 s | edit-rating single=$t3 s (hyperfine MISSING)"
  fi
  log "probe round=$round rows=$rows state=$state hyperfine: $summary"
}

# Runs alongside the scan: refreshes the row list and runs one hyperfine
# round every time the visible row count grows, then stops when all expected
# rows are visible or the scan settles.
probe_phase() {
  local waited=0 rows state last_rows="" rounds=0
  while :; do
    rows=0
    if build_probe_list; then
      rows="$(wc -l <"$OUT/probe-list.tsv" | tr -d ' ')"
    fi
    state="$(jq -r '.state // "unknown"' "$OUT/status-last.json" 2>/dev/null || echo unknown)"
    if [ "$rows" != "$last_rows" ] && [ "$rows" -gt 0 ]; then
      rounds=$((rounds + 1))
      probe_round "$rounds" "$rows" "$state" || log "probe round $rounds failed; continuing"
      last_rows="$rows"
    fi
    if [ -f "$OUT/scan-done.marker" ]; then break; fi
    if [ "$rows" -ge "$EXPECTED_ENTRIES" ]; then break; fi
    if [ "$SCAN_TIMEOUT_S" -gt 0 ] && [ "$waited" -ge "$SCAN_TIMEOUT_S" ]; then
      log "probe phase: rows=$rows state=$state waited~${waited}s (explicit SCAN_TIMEOUT_S), probing what exists"
      break
    fi
    sleep 0.5
    waited=$((waited + 1))
  done
  if [ "$rounds" -eq 0 ]; then
    log "probe phase: no image rows before the scan settled (state=$state)"
    return 0
  fi
  log "probe phase: rounds=$rounds rows=$(wc -l <"$OUT/probe-list.tsv" | tr -d ' ') state_at_break=$(jq -r '.state // "unknown"' "$OUT/status-last.json" 2>/dev/null || echo unknown)"
}

# --- scan (repeated; median wall time reported) ------------------------------
# SCAN_REPEATS runs of trigger→poll→drain on the same generated set; the
# median wall time is the reported scan_time (single runs are noisy).
# Repeats reprocess the same images through the full pipeline.
SCAN_REPEATS="${SCAN_REPEATS:-3}"
case "$SCAN_REPEATS" in ''|*[!0-9]*|0) die "SCAN_REPEATS must be a positive integer" ;; esac
log "scan repeats: $SCAN_REPEATS (median wall time reported)"
: >"$OUT/scan-walls.tsv"
rep=1
while [ "$rep" -le "$SCAN_REPEATS" ]; do
log "=== triggering index scan (run $rep/$SCAN_REPEATS) ==="
scan_start_ms="$(now_ms)"
code="$(curl -s -b "$COOKIES" -o "$OUT/index-trigger.json" -w '%{http_code}' -X POST \
  -H 'Content-Type: application/json' -d '{"album":"/"}' "$BASE/post/index/album")"
[ "$code" = "202" ] || { cat "$OUT/index-trigger.json" | tee -a "$LOG"; die "index trigger returned $code"; }
log "index trigger=202 start_ms=$scan_start_ms"
rm -f "$OUT/scan-done.marker"
ROUND_DIR="$OUT/rounds-run-$rep"
mkdir -p "$ROUND_DIR"

probe_phase >"$OUT/probe-phase.log" 2>&1 &
PROBE_PID=$!

log "=== polling /get/index/status ==="
state=""
if [ "$SCAN_TIMEOUT_S" -gt 0 ]; then
  deadline=$((SECONDS + SCAN_TIMEOUT_S))
  log "scan poll budget: ${SCAN_TIMEOUT_S}s (explicit SCAN_TIMEOUT_S)"
else
  deadline=0
  log "scan poll budget: none — waiting for a terminal state (completed/failed/canceled)"
fi
polls=0
poll_start_s=$SECONDS
while :; do
  curl -s -b "$COOKIES" -o "$OUT/status-last.json" "$BASE/get/index/status" || log "status poll failed"
  state="$(jq -r '.state // "unknown"' "$OUT/status-last.json" 2>/dev/null || echo unknown)"
  polls=$((polls + 1))
  case "$state" in
    completed | failed | canceled) break ;;
  esac
  if [ "$((polls % 1500))" -eq 0 ]; then
    log "still polling: state=$state polls=$polls elapsed=$((SECONDS - poll_start_s))s"
  fi
  if [ "$deadline" -gt 0 ] && [ "$SECONDS" -ge "$deadline" ]; then
    break
  fi
  sleep 0.2
done
scan_end_ms="$(now_ms)"
run_wall_ms=$((scan_end_ms - scan_start_ms))
cp "$OUT/status-last.json" "$OUT/status-final.json"
cp "$OUT/status-last.json" "$OUT/status-final-run-$rep.json"
printf 'state=%s wall_ms=%s\n' "$state" "$run_wall_ms" >"$OUT/scan-done.marker"
printf '%s\t%s\n' "$rep" "$run_wall_ms" >>"$OUT/scan-walls.tsv"
log "scan run $rep/$SCAN_REPEATS: state=$state polls=$polls wall_ms=$run_wall_ms"
log "status excerpt: $(jq -c '{state,scanned,matched,processed,failed,startedAt,finishedAt}' "$OUT/status-final.json")"

if [ -n "$PROBE_PID" ]; then
  wait "$PROBE_PID" || log "probe phase exited non-zero"
  PROBE_PID=""
fi
log "probe phase transcript: $OUT/probe-phase.log"

[ "$state" = "completed" ] || { jq . "$OUT/status-final.json" | tee -a "$LOG"; die "scan did not complete (state=$state)"; }
rep=$((rep + 1))
done
scan_wall_median_ms="$(sort -n -k2 "$OUT/scan-walls.tsv" | awk '{a[NR]=$2} END {if (NR%2==1) print a[(NR+1)/2]; else print (a[NR/2]+a[NR/2+1])/2}')"
log "scan wall median_ms=$scan_wall_median_ms over $SCAN_REPEATS run(s): $(cut -f2 "$OUT/scan-walls.tsv" | paste -sd, -)"

# Re-verify against the live endpoint: nothing post-scan (final sweep,
# hyperfine) may run unless the scan is terminally completed.
live_state="$(curl -s -b "$COOKIES" "$BASE/get/index/status" | jq -r '.state // "unknown"' 2>/dev/null || echo unknown)"
[ "$live_state" = "completed" ] || die "refusing post-scan work: live state=$live_state (expected completed)"
log "scan terminally completed — starting post-scan phase"

# Post-scan, off the scan path: one final full-list sequential sweep (ok-rate
# sanity) plus the single post-scan hyperfine run with the configured flags,
# then a read-back proving the write probe actually applied.
log "=== final sweep (sequential, post-scan) ==="
build_probe_list || die "could not build probe list after scan"
run_sequential_probes
[ -s "$OUT/probe-seq.tsv" ] || die "no sequential probe timings"
log "final sweep: $(probe_stats "$OUT/probe-seq.tsv" "seq")"
log "probe artifacts: $OUT/probe-seq.tsv $(wc -l <"$OUT/probe-seq.tsv" | tr -d ' ') rows"

# --- hyperfine -------------------------------------------------------------
# Post-scan baseline: compressed-get needs a verified-200 target (row order
# is arbitrary, and a video/unprocessed row would 404).
FIRST_HASH=""; FIRST_TOKEN=""
while read -r h t; do
  [ -n "$h" ] || continue
  code="$(curl -s -o /dev/null -b "$COOKIES" -H "Authorization: Bearer $t" \
    -w '%{http_code}' "$BASE/object/compressed/${h:0:2}/$h.jpg")" || continue
  if [ "$code" = "200" ]; then FIRST_HASH="$h"; FIRST_TOKEN="$t"; break; fi
done < <(head -n 8 "$OUT/probe-list.tsv")
[ -n "$FIRST_HASH" ] || die "no servable thumbnail for post-scan hyperfine"
CMD_COMPRESSED="curl -s -o /dev/null -b $COOKIES -H 'Authorization: Bearer $FIRST_TOKEN' -w '%{http_code} %{time_total}\n' $BASE/object/compressed/${FIRST_HASH:0:2}/$FIRST_HASH.jpg"
CMD_PREFETCH="curl -s -o /dev/null -b $COOKIES -X POST -H 'Content-Type: application/json' -d '{}' -w '%{http_code} %{time_total}\n' $BASE/get/prefetch"
# Write probe: rating 3 on snapshot index 0, idempotent across rounds (every
# run writes the same constant, so repeats converge instead of toggling
# state). Session cookie auth only (GuardUser+GuardMutate, no asset token);
# the route writes the sidecar, persists, and rebuilds the tree before 200.
TS_POST="$(jq -r '.prefetch.timestamp // empty' "$OUT/prefetch.json")"
[ -n "$TS_POST" ] || die "no prefetch timestamp for edit-rating probe"
CMD_EDIT_RATING="curl -s -o /dev/null -b $COOKIES -X PUT -H 'Content-Type: application/json' -d '{\"indexArray\":[0],\"timestamp\":$TS_POST,\"rating\":3}' -w '%{http_code} %{time_total}\n' $BASE/put/edit_rating"

log "=== hyperfine (warmup=$WARMUP runs=$REPEATS) ==="
if [ -n "$HYPERFINE_BIN" ]; then
  "$HYPERFINE_BIN" --warmup "$WARMUP" -r "$REPEATS" --export-json "$OUT/hyperfine.json" \
    -n "compressed-get" "$CMD_COMPRESSED" \
    -n "prefetch" "$CMD_PREFETCH" \
    -n "edit-rating" "$CMD_EDIT_RATING" 2>&1 | tee -a "$LOG"
  jq -e '.results | length == 3' "$OUT/hyperfine.json" >/dev/null || die "hyperfine json has no results"
  jq -r '.results[] | "hyperfine \(.name): mean=\(.summary.time_wall_clock.mean) s median=\(.summary.time_wall_clock.median) s stddev=\(.summary.time_wall_clock.stddev) runs=\(.summary.time_wall_clock.count) cmd=\(.command | gsub("Bearer [A-Za-z0-9._-]+"; "Bearer <redacted>"))"' \
    "$OUT/hyperfine.json" | tee -a "$LOG"
else
  : >"$OUT/hyperfine-fallback.txt"
  for ((r = 1; r <= REPEATS; r++)); do
    t="$(eval "$CMD_COMPRESSED" | awk '{print $2}')"
    printf 'run=%s compressed_get_s=%s\n' "$r" "$t" | tee -a "$LOG" >>"$OUT/hyperfine-fallback.txt"
  done
  log "hyperfine MISSING: wrote $OUT/hyperfine-fallback.txt"
fi

# Write-probe verification (once): the hyperfine 200s only prove the route
# answered. Re-read snapshot index 0 through the single-asset detail
# endpoint — list rows are lean and carry no rating — and require rating 3.
# A 200-without-apply here is a finding, not noise.
log "=== write-probe verification (rating == 3 on snapshot index 0) ==="
build_probe_list || die "write-probe verification: could not rebuild probe list"
VERIFY_TS="$(jq -r '.prefetch.timestamp // empty' "$OUT/prefetch.json")"
VERIFY_TOK="$(jq -r '.token // empty' "$OUT/prefetch.json")"
[ -n "$VERIFY_TS" ] && [ -n "$VERIFY_TOK" ] || die "write-probe verification: no prefetch timestamp/token"
code="$(curl -s -b "$COOKIES" -H "Authorization: Bearer $VERIFY_TOK" -o "$OUT/verify-row.json" -w '%{http_code}' \
  "$BASE/get/get-data?timestamp=$VERIFY_TS&start=0&end=1")" || die "write-probe verification: get-data curled"
[ "$code" = "200" ] || die "write-probe verification: get-data returned $code (expected 200)"
VERIFY_ASSET="$(jq -r '.[0].assetId // empty' "$OUT/verify-row.json")"
[ -n "$VERIFY_ASSET" ] || die "write-probe verification: no row at snapshot index 0"
code="$(curl -s -b "$COOKIES" -H "Authorization: Bearer $VERIFY_TOK" -o "$OUT/verify-metadata.json" -w '%{http_code}' \
  "$BASE/get/metadata/$VERIFY_ASSET?timestamp=$VERIFY_TS")" || die "write-probe verification: get-metadata curled"
[ "$code" = "200" ] || die "write-probe verification: get-metadata returned $code (expected 200)"
VERIFY_RATING="$(jq -r '.rating // empty' "$OUT/verify-metadata.json")"
[ "$VERIFY_RATING" = "3" ] || die "write-probe verification FAILED: rating=${VERIFY_RATING:-<unset>} on $VERIFY_ASSET (expected 3)"
log "write-probe verification: asset $VERIFY_ASSET rating=$VERIFY_RATING (expected 3)"

# --- summary ---------------------------------------------------------------
log "=== summary ==="
log "N=$N image_count=$IMG_COUNT image_bytes=$IMG_BYTES"
log "pool_sizes: single tokio RUNTIME workers=$N; global rayon max(1,N-1); server_threads_observed=$SERVER_THREADS"
log "scan_wall_ms=$scan_wall_median_ms scan_state=$state (median of $SCAN_REPEATS run(s))"
log "$(probe_stats "$OUT/probe-seq.tsv" "final-sweep")"
log "artifacts: $OUT"
log "done"
