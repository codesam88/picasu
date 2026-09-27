#!/usr/bin/env bash
# Reproduce the plan step 4 shape matrix: the Rocket/Tokio handler shapes the
# Picasu backend needs, measured on MIRAI 1.1.12.
#
#   tools/mirai-poc/run-shapes.sh            every case
#   tools/mirai-poc/run-shapes.sh spawn      only cases whose id contains "spawn"
#
# See ../README.md for the findings and the matrix. The script exits non-zero if
# a case stops matching, so it doubles as the gate for those claims.
#
# Differences from ../run.sh, all of them forced by the fixtures having
# dependencies:
#
# - The dependency-free fixture gets a fresh target directory per case, because
#   MIRAI is a rustc wrapper: if cargo considers the crate fresh it never invokes
#   MIRAI and the run silently reports nothing.
# - The two fixtures with dependencies share one target directory, because
#   rebuilding rocket per case would cost minutes. Instead every case touches
#   the fixture's source file first, and each run asserts that MIRAI really
#   re-analyzed the selected function (the LIVE column). A STALE row is a failed
#   run, never a result.
# - `TMPDIR` is redirected at the scratch directory for every cargo and MIRAI
#   invocation. rustc and MIRAI's summary store both write their temporary files
#   under the system temp directory, and on a machine with a small `/tmp` quota
#   that shows up as `Disk quota exceeded` and, for MIRAI, as an internal panic in
#   `SummaryCache::create_summary_store_if_needed` with a 101 exit.
# - `time` and `encoding_rs` are pinned back to the newest versions that build on
#   MIRAI's pinned compiler. Their current versions declare `rust-version = 1.88`
#   and rocket 0.5.1 depends on both, so without this the fixture does not build
#   at all. The pins are applied to the scratch copy only.
#
# Requires: endorlabs/MIRAI v1.1.12 installed as `cargo-mirai`/`mirai`, the
# nightly-2025-01-10 toolchain with the rustc-dev and rust-src components, and
# network access for the first run (the fixtures have dependencies).

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work="${MIRAI_POC_WORK:-/tmp/opencode/mirai-poc}"
toolchain="${MIRAI_POC_TOOLCHAIN:-nightly-2025-01-10}"
expected_version="${MIRAI_POC_VERSION:-1.1.12}"
filter="${1:-}"

# Newest versions that the pinned nightly (rustc 1.86.0-nightly) accepts.
msrv_pins=(
  "time@0.3.45"        # newest 0.3.x with rust-version <= 1.86; rocket 0.5.1 needs it
  "time-core@0.1.7"
  "time-macros@0.2.25"
  "encoding_rs@0.8.35" # newest with rust-version <= 1.86
)

# <fixture> <diag> <single_func|*> <expected src diagnostics> <required message> <forbidden message>
# `*` as the function analyzes the whole crate, and `*` as the expected count
# means "at least one, the count is not stable".
cases=(
  "fixture-handler|verify|*|3||"
  "fixture-handler|paranoid|*|18||"
  "fixture-handler|paranoid|h1_query_primitive_to_helper_unwrap|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|paranoid|h2_query_primitive_to_helper_expect|0||"
  "fixture-handler|paranoid|h21_query_primitive_to_result_expect|1|possible result unwrap failed|"
  "fixture-handler|paranoid|h3_query_index_to_helper_index|1|possible index out of bounds|"
  "fixture-handler|paranoid|h4_static_method_result_unwrap|1|possible result unwrap failed|"
  "fixture-handler|paranoid|h5_query_primitive_division|1|possible attempt to divide by zero|"
  "fixture-handler|paranoid|h6_json_body_into_inner_unwrap|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|paranoid|h7_json_body_deref_unwrap|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|paranoid|h8_json_body_deref_index|1|possible index out of bounds|"
  "fixture-handler|paranoid|h9_json_body_index_into_helper|1|possible index out of bounds|"
  "fixture-handler|paranoid|h10_guard_discarded|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|paranoid|h11_guard_propagated|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|paranoid|h12_guard_discarded_no_sink|0||"
  "fixture-handler|paranoid|h13_guard_propagated_no_sink|0||"
  "fixture-handler|paranoid|h19_guard_discarded_definite_sink|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|paranoid|h20_guard_propagated_definite_sink|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|paranoid|h14_in_crate_spawn_like_unwrap|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|paranoid|h15_in_crate_spawn_like_index|1|possible index out of bounds|"
  "fixture-handler|paranoid|h16_std_thread_spawn_unwrap|2|possible result unwrap failed|called \`Option::unwrap()\` on a \`None\` value"
  "fixture-handler|paranoid|h17_async_handler_sync_sink|0||"
  "fixture-handler|paranoid|h18_async_block_never_polled|0||"
  "fixture-handler|paranoid|h22_uncalled_closure_concrete_sink|0||"
  "fixture-handler|paranoid|h23_called_closure_concrete_sink|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|paranoid|h24_async_concrete_sink|0||"
  "fixture-handler|verify|h22_uncalled_closure_concrete_sink|0||"
  "fixture-handler|verify|h23_called_closure_concrete_sink|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|verify|h24_async_concrete_sink|0||"
  "fixture-handler|verify|h10_guard_discarded|0||"
  "fixture-handler|verify|h11_guard_propagated|0||"
  "fixture-handler|verify|h13_guard_propagated_no_sink|0||"
  "fixture-handler|verify|h19_guard_discarded_definite_sink|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|verify|h20_guard_propagated_definite_sink|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-handler|verify|h16_std_thread_spawn_unwrap|0||"
  "fixture-handler|verify|h14_in_crate_spawn_like_unwrap|0||"
  "fixture-handler|verify|h17_async_handler_sync_sink|0||"
  "fixture-handler|verify|*|3||"
  "fixture-tokio|paranoid|*|2||"
  "fixture-tokio|paranoid|k1_spawn_blocking_closure_unwrap|0||"
  "fixture-tokio|paranoid|k2_spawn_blocking_closure_expect|0||"
  "fixture-tokio|paranoid|k3_spawn_blocking_closure_index|0||"
  "fixture-tokio|paranoid|k4_spawn_blocking_closure_calls_helper|0||"
  "fixture-tokio|paranoid|k5_sync_closure_called_directly|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-tokio|paranoid|k6_sync_helper_sink|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-tokio|paranoid|k7_async_await_handle_unwrap|0||"
  "fixture-tokio|paranoid|k8_async_get_rows_shape|0||"
  "fixture-tokio|verify|*|0||"
  "fixture-tokio|verify|k1_spawn_blocking_closure_unwrap|0||"
  "fixture-tokio|verify|k5_sync_closure_called_directly|0||"
  "fixture-tokio|verify|k7_async_await_handle_unwrap|0||"
  "fixture-rocket|paranoid|*|12|incomplete analysis of call|"
  "fixture-rocket|paranoid|r1_get_primitive_to_helper_unwrap|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-rocket|paranoid|r2_get_guard_propagated_result_unwrap|1|possible result unwrap failed|"
  "fixture-rocket|paranoid|r3_get_guard_propagated_expect|1|possible result unwrap failed|"
  "fixture-rocket|paranoid|r4_post_json_body_unwrap|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture-rocket|paranoid|r5_post_json_body_index|1|possible index out of bounds|"
  "fixture-rocket|paranoid|r6_get_async_spawn_blocking_helper|0||"
  "fixture-rocket|paranoid|r7_get_async_spawn_blocking_closure|0||"
  "fixture-rocket|verify|r1_get_primitive_to_helper_unwrap|0||"
  "fixture-rocket|verify|r4_post_json_body_unwrap|0||"
  "fixture-rocket|verify|r6_get_async_spawn_blocking_helper|0||"
)

# The first blocker of the rocket experiment, isolated. The fixture has two
# dependencies and one empty function, and the failure happens before any
# analysis: cargo refuses to resolve a dependency graph whose MSRV is newer than
# MIRAI's pinned compiler. `pinned` is the same resolve after the pins above, so
# the pair brackets the workaround as well.
#
# <fixture> <variant> <expected exit> <required message>
blockers=(
  "fixture-msrv|resolve|101|is not supported by the following packages"
  "fixture-msrv|pinned|0||"
)

fatal() { printf 'error: %s\n' "$1" >&2; exit 1; }

command -v cargo >/dev/null || fatal "cargo not on PATH"
rustup toolchain list 2>/dev/null | grep -q "^$toolchain" \
  || fatal "toolchain $toolchain not installed (rustup toolchain install $toolchain --component rustc-dev --component rust-src)"

installed="$(cargo mirai --version 2>&1 | awk '{print $2}')"
[ "$installed" = "$expected_version" ] \
  || fatal "expected MIRAI $expected_version, found '${installed:-<none>}'; the results in README.md are version specific"
printf 'MIRAI %s, toolchain %s (%s), scratch %s\n\n' \
  "$installed" "$toolchain" "$(rustc "+$toolchain" --version)" "$work"

# Copy the fixtures out of the repository. They are never built in place.
rm -rf "$work/fixture-handler" "$work/fixture-tokio" "$work/fixture-rocket" \
  "$work/fixture-msrv" "$work/target" "$work/logs"
mkdir -p "$work/logs"
cp -r "$here/fixture-handler" "$here/fixture-tokio" "$here/fixture-rocket" \
  "$here/fixture-msrv" "$work/"

# The dependency fixtures need a lock file before `cargo update --precise` works.
# Only the rocket fixture needs the pins; tokio has no MSRV-conflicting
# dependency, and asking for a package that is not in the graph is an error.
prepare() {
  local fixture="$1"
  shift
  local log="$work/logs/$fixture-resolve.log"
  ( cd "$work/$fixture" \
    && TMPDIR="$work" cargo "+$toolchain" generate-lockfile ) >"$log" 2>&1
  [ -f "$work/$fixture/Cargo.lock" ] || fatal "$fixture: no lock file, see $log"
  for pin in "$@"; do
    ( cd "$work/$fixture" \
      && TMPDIR="$work" cargo "+$toolchain" update -p "${pin%@*}" --precise "${pin#*@}" ) \
      >>"$log" 2>&1 || fatal "$fixture: could not pin $pin, see $log"
  done
}
prepare fixture-tokio
prepare fixture-rocket "${msrv_pins[@]}"

# Emits one "<message> @ <span>" line per MIRAI diagnostic whose primary span is
# in the fixture's own source. Everything else is standard-library noise that
# `--diag=paranoid` produces and that a gate has to filter out.
src_diagnostics() {
  awk '
    /warning: \[MIRAI\]/ {
      msg = $0; sub(/^warning: \[MIRAI\] /, "", msg)
      span = ""; want = 1; next
    }
    want && /^ *--> / {
      if (span == "") { span = $0; sub(/^ *--> /, "", span) }
      if (span ~ /^src\//) print msg " @ " span
      want = 0; next
    }
    want && (/^$/ || /^note:/ || /^ *=/ || /^ *\|/) { next }
    want { want = 0 }
  ' "$1"
}

# Echoes "<exit code> <milliseconds>"; the output goes to the log file. A fresh
# target directory for the dependency-free fixture, a touched source file for the
# other two (see the header).
run_mirai() {
  local fixture="$1" level="$2" func="$3" log="$4"
  local flags="--diag=$level"
  [ "$func" != '*' ] && flags="$flags --single_func $func"
  local dir start end code
  if [ "$fixture" = fixture-handler ]; then
    dir="$work/target/$fixture-$level-$func"
    rm -rf "$dir"
  else
    dir="$work/target/$fixture"
    touch "$work/$fixture/src/lib.rs"
  fi
  start=$(date +%s%N)
  ( cd "$work/$fixture" \
    && CARGO_TARGET_DIR="$dir" TMPDIR="$work" MIRAI_LOG=info MIRAI_FLAGS="$flags" \
       cargo "+$toolchain" mirai --lib ) >"$log" 2>&1
  code=$?
  end=$(date +%s%N)
  printf '%s %s' "$code" "$(( (end - start) / 1000000 ))"
}

failures=0

# The blocker checks run before the analysis matrix: they are cheap and they
# decide whether the dependency fixtures can be resolved at all.
printf '%-42s %5s %-6s %s\n' BLOCKER EXIT LIVE RESULT
for spec in "${blockers[@]}"; do
  IFS='|' read -r fixture variant want_code want_msg <<<"$spec"
  case_id="$fixture:$variant"
  [ -n "$filter" ] && [[ "$case_id" != *"$filter"* ]] && continue

  log="$work/logs/${case_id//:/-}.log"
  dir="$work/$fixture"
  rm -f "$dir/Cargo.lock"
  ( cd "$dir" && TMPDIR="$work" cargo "+$toolchain" generate-lockfile ) >"$log" 2>&1
  if [ "$variant" = pinned ]; then
    for pin in "${msrv_pins[@]}"; do
      ( cd "$dir" && TMPDIR="$work" \
        && cargo "+$toolchain" update -p "${pin%@*}" --precise "${pin#*@}" ) >>"$log" 2>&1
    done
  fi
  # The resolver only annotates the offending packages ("Adding time v0.3.55
  # (requires Rust 1.88.0)"); the hard error comes from compiling, so that is what
  # the blocker check runs.
  ( cd "$dir" && TMPDIR="$work" cargo "+$toolchain" check --lib ) >>"$log" 2>&1
  code=$?

  ok=1
  [ "$code" -eq "$want_code" ] || ok=0
  { [ -z "$want_msg" ] || grep -qF -- "$want_msg" "$log"; } || ok=0
  if [ "$ok" -eq 1 ]; then
    result=ok
  else
    result=MISMATCH
    failures=$((failures + 1))
  fi
  printf '%-42s %5s %-6s %s\n' "$case_id" "$code" n/a "$result"
  [ "$result" = ok ] || grep -hE '^(error|note: select)' "$log" | head -4 | sed 's/^/      /'
done
printf '\n'

printf '%-42s %-8s %5s %4s %4s %8s %-6s %s\n' \
  CASE DIAG EXIT SRC ALL MS LIVE RESULT
for spec in "${cases[@]}"; do
  IFS='|' read -r fixture level func want_n want_msg want_none <<<"$spec"
  case_id="$fixture:${func:-crate}"
  [ -n "$filter" ] && [[ "$case_id" != *"$filter"* ]] && continue

  log="$work/logs/${fixture}-${level}-${func//\*/crate}.log"
  read -r code ms <<<"$(run_mirai "$fixture" "$level" "$func" "$log")"

  # The liveness check. Without it a stale cargo fingerprint looks exactly like a
  # clean result: MIRAI is a rustc wrapper, so a fresh crate is never analyzed.
  if [ "$func" = '*' ]; then
    grep -q 'analyzing function ' "$log" && live=yes || live=STALE
  else
    grep -q "analyzing .*function .*$func\$" "$log" && live=yes || live=STALE
  fi

  mapfile -t diags < <(src_diagnostics "$log")
  n=${#diags[@]}
  n_all=$(grep -c '\[MIRAI\]' "$log")

  count_ok=1
  if [ "$want_n" = '*' ]; then
    [ "$n" -ge 1 ] || count_ok=0
  else
    [ "$n" -eq "$want_n" ] || count_ok=0
  fi
  msg_ok=1
  { [ -z "$want_msg" ] || grep -qF -- "$want_msg" "$log"; } || msg_ok=0
  { [ -z "$want_none" ] || ! grep -qF -- "$want_none" "$log"; } || msg_ok=0
  # MIRAI never exits non-zero for its own diagnostics, so a gate has to scrape
  # the output. Assert that rather than relying on it.
  exit_ok=1
  [ "$code" -eq 0 ] || exit_ok=0

  if [ "$count_ok$msg_ok$exit_ok" = 111 ] && [ "$live" = yes ]; then
    result=ok
  else
    result=MISMATCH
    failures=$((failures + 1))
  fi
  printf '%-42s %-8s %5s %4s %4s %7sms %-6s %s\n' \
    "${case_id#fixture-}" "$level" "$code" "$n" "$n_all" "$ms" "$live" "$result"
  for d in "${diags[@]}"; do
    printf '      %s\n' "$d"
  done
  if [ "$live" != yes ]; then
    printf '      MIRAI did not analyze this run: stale cargo fingerprint\n'
  fi
  if [ "$exit_ok" = 0 ]; then
    printf '      cargo exited %s, expected 0\n' "$code"
  fi
done

cat <<'note'

MIRAI never exits non-zero for its own diagnostics, so EXIT is 0 whether or not
SRC is greater than zero. The SRC column counts only the diagnostics whose span
is in the fixture; ALL also counts the standard-library noise that --diag=paranoid
adds. LIVE is the check that the run was real.
note
printf '%s case(s) did not match README.md\n' "$failures"
[ "$failures" -eq 0 ]
