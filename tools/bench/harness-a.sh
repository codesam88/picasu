#!/usr/bin/env bash
# Harness A: baseline measurement for the worker-pool rework.
#
# Generates a jpg-only snapfab image set, boots the server under a fixed CPU
# budget, triggers an album index scan, polls it to a terminal state, and
# measures fetch latency (sequential + concurrent) plus a hyperfine repeat run.
#
# Usage:
#   tools/bench/harness-a.sh
#
# Environment:
#   N            cores for the server's taskset budget (default 4)
#   COUNT        image count when generating a manifest (default 10)
#   REPEATS      hyperfine runs (default 3)
#   WARMUP       hyperfine warmup runs (default 1)
#   CONCURRENCY  xargs -P width for the concurrent probe (default: N)
#   MANIFEST     PhotoSpec manifest to use instead of generating one (optional)
#   OUT          output directory (default /tmp/opencode/worker-pool-rework/harness-sanity)
#   PORT         server port (default 5713)
#   PROBE_LIMIT  max compressed files probed (default: COUNT)
#   SCAN_TIMEOUT_S  scan poll budget in seconds (default 1800)
#
# All artifacts land in $OUT: server log, status snapshots, probe TSVs,
# hyperfine JSON, and harness-a.log (the run transcript).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

N="${N:-4}"
COUNT="${COUNT:-10}"
REPEATS="${REPEATS:-3}"
WARMUP="${WARMUP:-1}"
CONCURRENCY="${CONCURRENCY:-$N}"
PORT="${PORT:-5713}"
OUT="${OUT:-/tmp/opencode/worker-pool-rework/harness-sanity}"
MANIFEST="${MANIFEST:-}"
PROBE_LIMIT="${PROBE_LIMIT:-$COUNT}"
SCAN_TIMEOUT_S="${SCAN_TIMEOUT_S:-1800}"
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
log "=== building binaries ==="
( cd "$REPO_ROOT" && cargo build --bin picasu ) >"$OUT/build-picasu.log" 2>&1 \
  || { tail -20 "$OUT/build-picasu.log" | tee -a "$LOG"; die "cargo build --bin picasu failed"; }
( cd "$REPO_ROOT" && cargo build -p snapfab ) >"$OUT/build-snapfab.log" 2>&1 \
  || { tail -20 "$OUT/build-snapfab.log" | tee -a "$LOG"; die "cargo build -p snapfab failed"; }
PICASU_BIN="$REPO_ROOT/target/debug/picasu"
SNAPFAB_BIN="$REPO_ROOT/target/debug/snapfab"
[ -x "$PICASU_BIN" ] || die "missing $PICASU_BIN"
[ -x "$SNAPFAB_BIN" ] || die "missing $SNAPFAB_BIN"
log "picasu=$PICASU_BIN snapfab=$SNAPFAB_BIN"

# --- image set -------------------------------------------------------------
IMG_DIR="$OUT/images"
mkdir -p "$IMG_DIR"
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

log "=== generating images ==="
gen_start_ms="$(now_ms)"
( cd "$IMG_DIR" && "$SNAPFAB_BIN" --verbose batch "$MANIFEST_PATH" ) 2>&1 | tee -a "$LOG"
gen_end_ms="$(now_ms)"
IMG_COUNT="$(find "$IMG_DIR" -maxdepth 1 -type f -name '*.jpg' | wc -l | tr -d ' ')"
IMG_BYTES="$(find "$IMG_DIR" -maxdepth 1 -type f -name '*.jpg' -printf '%s\n' | awk '{s += $1} END {print s + 0}')"
[ "$IMG_COUNT" -eq "$EXPECTED_ENTRIES" ] || die "expected $EXPECTED_ENTRIES images, found $IMG_COUNT"
log "generated=$IMG_COUNT bytes=$IMG_BYTES wall_ms=$((gen_end_ms - gen_start_ms))"

# --- run parameters (echoed into the log) ----------------------------------
log "=== run parameters ==="
log "N=$N taskset_cores=0-$((N - 1)) nproc_under_affinity=$(taskset -c "0-$((N - 1))" nproc)"
log "image_count=$IMG_COUNT image_bytes=$IMG_BYTES"
log "pool_sizes: CURRENT_NUM_THREADS=$N -> single tokio RUNTIME worker_threads=$N; global rayon pool max(1,N-1)"
log "hyperfine=$HYPERFINE_BIN warmup=$WARMUP repeats=$REPEATS concurrency=$CONCURRENCY port=$PORT"

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

run_concurrent_probes() {
  : >"$OUT/probe-conc.tsv"
  export BASE COOKIES
  export CONC_OUT="$OUT/probe-conc.tsv"
  xargs -P "$CONCURRENCY" -n 2 -a "$OUT/probe-list.tsv" bash -c '
    hash=$1
    token=$2
    res=$(curl -s -o /dev/null -b "$COOKIES" -H "Authorization: Bearer $token" \
      -w "%{http_code}\t%{time_total}" \
      "$BASE/object/compressed/${hash:0:2}/$hash.jpg") || res=$(printf "curl_error\t0")
    printf "%s\t%s\n" "$hash" "$res" >>"$CONC_OUT"
  ' _
}

probe_stats() {
  awk -F'\t' -v label="$2" '
    { n++; t = $3 + 0; sum += t; if (n == 1 || t < min) min = t; if (t > max) max = t; if ($2 == "200") ok++ }
    END {
      if (n > 0) printf "%s: n=%d ok=%d mean=%.6f min=%.6f max=%.6f\n", label, n, ok + 0, sum / n, min, max
      else printf "%s: EMPTY\n", label
    }' "$1"
}

# One probe round over the rows visible now: sequential, then concurrent, each
# round also copied to round-tagged files so the during-scan rounds stay
# inspectable. The canonical probe-seq.tsv / probe-conc.tsv always hold the
# latest (largest-coverage) round.
probe_round() {
  local round="$1" rows="$2" state="$3"
  run_sequential_probes
  run_concurrent_probes
  cp "$OUT/probe-seq.tsv" "$OUT/probe-seq-round-$round.tsv"
  cp "$OUT/probe-conc.tsv" "$OUT/probe-conc-round-$round.tsv"
  log "probe round=$round rows=$rows state=$state seq: $(probe_stats "$OUT/probe-seq.tsv" "seq") conc: $(probe_stats "$OUT/probe-conc.tsv" "conc-P$CONCURRENCY")"
}

# Runs alongside the scan: refreshes the row list and probes every time the
# visible row count grows, then stops when all expected rows are visible or
# the scan settles.
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
      probe_round "$rounds" "$rows" "$state"
      last_rows="$rows"
    fi
    if [ -f "$OUT/scan-done.marker" ]; then break; fi
    if [ "$rows" -ge "$EXPECTED_ENTRIES" ]; then break; fi
    if [ "$waited" -ge "$SCAN_TIMEOUT_S" ]; then
      log "probe phase: rows=$rows state=$state waited=${waited}s, probing what exists"
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

# --- scan ------------------------------------------------------------------
log "=== triggering index scan ==="
scan_start_ms="$(now_ms)"
code="$(curl -s -b "$COOKIES" -o "$OUT/index-trigger.json" -w '%{http_code}' -X POST \
  -H 'Content-Type: application/json' -d '{"album":"/"}' "$BASE/post/index/album")"
[ "$code" = "202" ] || { cat "$OUT/index-trigger.json" | tee -a "$LOG"; die "index trigger returned $code"; }
log "index trigger=202 start_ms=$scan_start_ms"

probe_phase >"$OUT/probe-phase.log" 2>&1 &
PROBE_PID=$!

log "=== polling /get/index/status ==="
state=""
deadline=$((SECONDS + SCAN_TIMEOUT_S))
polls=0
while :; do
  curl -s -b "$COOKIES" -o "$OUT/status-last.json" "$BASE/get/index/status" || log "status poll failed"
  state="$(jq -r '.state // "unknown"' "$OUT/status-last.json" 2>/dev/null || echo unknown)"
  polls=$((polls + 1))
  case "$state" in
    completed | failed | canceled) break ;;
  esac
  if [ "$SECONDS" -ge "$deadline" ]; then
    break
  fi
  sleep 0.2
done
scan_end_ms="$(now_ms)"
cp "$OUT/status-last.json" "$OUT/status-final.json"
printf 'state=%s wall_ms=%s\n' "$state" "$((scan_end_ms - scan_start_ms))" >"$OUT/scan-done.marker"
log "scan state=$state polls=$polls wall_ms=$((scan_end_ms - scan_start_ms))"
log "status excerpt: $(jq -c '{state,scanned,matched,processed,failed,startedAt,finishedAt}' "$OUT/status-final.json")"
log "scan json: startedAt=$(jq -r .startedAt "$OUT/status-final.json") finishedAt=$(jq -r .finishedAt "$OUT/status-final.json")"

if [ -n "$PROBE_PID" ]; then
  wait "$PROBE_PID" || log "probe phase exited non-zero"
  PROBE_PID=""
fi
log "probe phase transcript: $OUT/probe-phase.log"

[ "$state" = "completed" ] || { jq . "$OUT/status-final.json" | tee -a "$LOG"; die "scan did not complete (state=$state)"; }

# Fallback: if the in-flight probe phase produced nothing, probe after the scan.
if [ ! -s "$OUT/probe-seq.tsv" ]; then
  log "probe phase produced no rows; probing post-scan"
  build_probe_list || die "could not build probe list after scan"
  run_sequential_probes
  log "sequential probes: $(probe_stats "$OUT/probe-seq.tsv" "seq")"
  run_concurrent_probes
  log "concurrent probes: $(probe_stats "$OUT/probe-conc.tsv" "xargs -P $CONCURRENCY")"
fi
[ -s "$OUT/probe-seq.tsv" ] || die "no sequential probe timings"
[ -s "$OUT/probe-conc.tsv" ] || die "no concurrent probe timings"
log "probe artifacts: $OUT/probe-seq.tsv $(wc -l <"$OUT/probe-seq.tsv" | tr -d ' ') rows; $OUT/probe-conc.tsv $(wc -l <"$OUT/probe-conc.tsv" | tr -d ' ') rows"

# --- hyperfine -------------------------------------------------------------
FIRST_HASH="$(head -n 1 "$OUT/probe-list.tsv" | cut -f1)"
FIRST_TOKEN="$(head -n 1 "$OUT/probe-list.tsv" | cut -f2)"
CMD_COMPRESSED="curl -s -o /dev/null -b $COOKIES -H 'Authorization: Bearer $FIRST_TOKEN' -w '%{http_code} %{time_total}\n' $BASE/object/compressed/${FIRST_HASH:0:2}/$FIRST_HASH.jpg"
CMD_PREFETCH="curl -s -o /dev/null -b $COOKIES -X POST -H 'Content-Type: application/json' -d '{}' -w '%{http_code} %{time_total}\n' $BASE/get/prefetch"

log "=== hyperfine (warmup=$WARMUP runs=$REPEATS) ==="
if [ -n "$HYPERFINE_BIN" ]; then
  "$HYPERFINE_BIN" --warmup "$WARMUP" -r "$REPEATS" --export-json "$OUT/hyperfine.json" \
    -n "compressed-get" "$CMD_COMPRESSED" \
    -n "prefetch" "$CMD_PREFETCH" 2>&1 | tee -a "$LOG"
  jq -e '.results | length > 0' "$OUT/hyperfine.json" >/dev/null || die "hyperfine json has no results"
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

# --- summary ---------------------------------------------------------------
log "=== summary ==="
log "N=$N image_count=$IMG_COUNT image_bytes=$IMG_BYTES"
log "pool_sizes: single tokio RUNTIME workers=$N; global rayon max(1,N-1); server_threads_observed=$SERVER_THREADS"
log "scan_wall_ms=$((scan_end_ms - scan_start_ms)) scan_state=$state"
log "$(probe_stats "$OUT/probe-seq.tsv" "seq")"
log "$(probe_stats "$OUT/probe-conc.tsv" "conc-P$CONCURRENCY")"
log "artifacts: $OUT"
log "done"
