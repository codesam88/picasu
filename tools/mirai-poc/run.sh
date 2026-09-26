#!/usr/bin/env bash
# Reproduce the MIRAI 1.1.12 reachability baseline for the Picasu backend.
#
# See README.md for the findings. Nothing is written inside the repository: the
# fixtures are copied to $WORK and every MIRAI run gets its own target
# directory. The fresh target directory is not cosmetic. MIRAI is a rustc
# wrapper, so when cargo considers the crate fresh it never invokes MIRAI and
# the run silently reports nothing.
#
# Usage:
#   tools/mirai-poc/run.sh            run every case and check the expectations
#   MIRAI_POC_WORK=/tmp/x run.sh      use a different scratch directory
#
# Requires: endorlabs/MIRAI v1.1.12 installed as `cargo-mirai`/`mirai`, and the
# nightly-2025-01-10 toolchain with the rustc-dev and rust-src components.

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work="${MIRAI_POC_WORK:-/tmp/opencode/mirai-poc}"
toolchain="${MIRAI_POC_TOOLCHAIN:-nightly-2025-01-10}"
expected_version="${MIRAI_POC_VERSION:-1.1.12}"

# <fixture> <diag level> <single_func> <expected count> <required message> <forbidden message>
# An empty <single_func> analyzes the whole crate, and `*` as the expected count
# means "at least one, the exact count is not stable". `single_func` keeps
# attribution unambiguous: a whole-crate run reports several cases at once and
# the only way to tell them apart is the span.
cases=(
  "fixture|default||0||"
  "fixture|verify||0||"
  "fixture|paranoid||*|possible index out of bounds|"
  "fixture|paranoid|case_a_request_unwrap|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture|paranoid|case_b_config_invariant|0||"
  "fixture|paranoid|case_c_helper|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture|paranoid|case_d_move_closure|1|called \`Option::unwrap()\` on a \`None\` value|"
  "fixture|paranoid|case_e_spawn_blocking|6|possible result unwrap failed|called \`Option::unwrap()\` on a \`None\` value"
  "fixture|paranoid|case_f_index_out_of_bounds|1|possible index out of bounds|"
  "fixture|paranoid|case_g_explicit_panic|0||"
  "fixture|paranoid|case_h_request_expect|0||"
  "fixture|paranoid|case_i_request_result_unwrap|1|possible result unwrap failed|"
  "fixture-taint|verify|t1_tagged_into_helper|0||"
  "fixture-taint|verify|t2_tagged_into_closure|0||"
  "fixture-taint|verify|t3_untagged_control|0||"
  "fixture-taint|verify|t4_sanitizer_is_a_no_op|0||"
  "fixture-taint|verify|t5_tagged_field|1|unsatisfied precondition|"
  "fixture-taint|verify|t6_taint_does_not_survive_rebinding|0||"
  "fixture-taint|verify|t7_tagged_local_direct_sink|0||"
  "fixture-taint|verify|t8_intervening_ref_call|0||"
  "fixture-taint|verify|t9_inline_sink|0||"
  "fixture-taint|verify|t10_positive_check_on_local|1|possible false verification condition|"
  "fixture-taint|verify|t11_tag_param_pass_by_value|1|unsatisfied precondition|"
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
rm -rf "$work/fixture" "$work/fixture-taint" "$work/target" "$work/logs"
cp -r "$here/fixture" "$here/fixture-taint" "$work/"
mkdir -p "$work/logs"

# Echoes "<exit code> <milliseconds>"; the output goes to the log file.
run_mirai() {
  local fixture="$1" level="$2" func="$3" dir="$4" log="$5"
  local flags="--diag=$level"
  [ -n "$func" ] && flags="$flags --single_func $func"
  rm -rf "$dir"
  local start end code
  start=$(date +%s%N)
  ( cd "$work/$fixture" \
    && CARGO_TARGET_DIR="$dir" MIRAI_FLAGS="$flags" cargo "+$toolchain" mirai --lib ) \
    >"$log" 2>&1
  code=$?
  end=$(date +%s%N)
  printf '%s %s' "$code" "$(( (end - start) / 1000000 ))"
}

failures=0
printf '%-14s %-8s %-32s %5s %5s %7s  %s\n' \
  FIXTURE DIAG FUNCTION DIAGS EXIT MS RESULT
for spec in "${cases[@]}"; do
  IFS='|' read -r fixture level func want_n want_msg want_none <<<"$spec"
  tag="${func:-$fixture(crate)}"
  log="$work/logs/${fixture}-${level}-${func:-crate}.log"

  read -r code ms <<<"$(run_mirai "$fixture" "$level" "$func" \
    "$work/target/${fixture}-${level}-${func:-crate}" "$log")"

  n=$(grep -c '\[MIRAI\]' "$log")
  count_ok=1
  if [ "$want_n" = '*' ]; then
    [ "$n" -ge 1 ] || count_ok=0
  else
    [ "$n" -eq "$want_n" ] || count_ok=0
  fi
  msg_ok=1
  { [ -z "$want_msg" ] || grep -qF -- "$want_msg" "$log"; } || msg_ok=0
  { [ -z "$want_none" ] || ! grep -qF -- "$want_none" "$log"; } || msg_ok=0

  if [ "$count_ok" -eq 1 ] && [ "$msg_ok" -eq 1 ]; then
    result=ok
  else
    result=MISMATCH
    failures=$((failures + 1))
  fi
  printf '%-14s %-8s %-32s %5s %5s %6sms  %s\n' \
    "$fixture" "$level" "$tag" "$n" "$code" "$ms" "$result"
  grep -hF '[MIRAI]' "$log" | sed 's/warning: \[MIRAI\] /      /' | sort -u | sed 's/^ *//'
done

cat <<'note'

MIRAI never exits non-zero for its own diagnostics, so a gate has to scrape the
output rather than rely on the exit status.
note
printf '%s case(s) did not match README.md\n' "$failures"
[ "$failures" -eq 0 ]
