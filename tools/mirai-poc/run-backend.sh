#!/usr/bin/env bash
# Plan step 5: the integration boundary. Three runs that do not fit the
# shape matrix in ../run-shapes.sh, because each of them is expected to fail.
#
#   tools/mirai-poc/run-backend.sh
#
# 1. The real backend package, with the exact command a gate would use. The
#    workspace is copied out of the repository first, so nothing here can touch
#    the repository's Cargo.lock, toolchain or target directory.
# 2. The same copy with every MSRV pin MIRAI's compiler would need, to show how
#    deep the blocker goes. The pins are refused by version *requirements*, not
#    only by the MSRV, so the workaround is a manifest and code change rather
#    than a lock-file change.
# 3. A four-line function that makes MIRAI itself abort, so a gate that scrapes
#    `[MIRAI]` lines cannot mistake a tool failure for a clean result.
#
# See ../README.md for the findings. The script exits non-zero if a run stops
# matching, so it doubles as the gate for those claims.
#
# Requires: endorlabs/MIRAI v1.1.12 installed as `cargo-mirai`/`mirai`, the
# nightly-2025-01-10 toolchain with the rustc-dev and rust-src components, and
# network access for the pin attempt.

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work="${MIRAI_POC_WORK:-/tmp/opencode/mirai-poc}"
toolchain="${MIRAI_POC_TOOLCHAIN:-nightly-2025-01-10}"
expected_version="${MIRAI_POC_VERSION:-1.1.12}"

# Newest version of each package the root lock resolves to whose declared MSRV
# the pinned nightly (rustc 1.86.0-nightly) accepts, read out of the local
# crates.io index cache. The second column is what the backend's own manifest
# asks for; where the two disagree the pin is impossible without editing
# backend/Cargo.toml.
pins=(
  "built@0.8.0"             # transitive, patch-level
  "encoding_rs@0.8.35"      # transitive, patch-level
  "image@0.25.9"            # backend asks for ^0.25.10
  "image_hasher@3.0.0"      # backend asks for ^3.1.1
  "jsonwebtoken@10.3.0"     # backend asks for ^11.0.0, major downgrade
  "redb@2.6.3"              # backend asks for ^4.1, two majors down
  "time@0.3.45"             # cookie_store 0.22.1 asks for ^0.3.47
  "time-core@0.1.7"
  "time-macros@0.2.25"
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

# The repository is only ever read. `cp -r` of the workspace manifest, the lock
# file, the cargo config, the backend crate and the two path crates is enough to
# resolve and build the backend; the lock file is what the blocker is about, so
# it is the repository's own, not a regenerated one.
ws="$work/backend-ws"
rm -rf "$ws" "$work/backend-target" "$work/backend-logs" "$work/.tmp*"
mkdir -p "$ws" "$work/backend-logs"
cp -r "$here/../../Cargo.toml" "$here/../../Cargo.lock" "$here/../../.cargo" "$ws/"
cp -r "$here/../../backend" "$here/../../utils" "$ws/"
repo_lock_sum="$(sha256sum "$here/../../Cargo.lock" | awk '{print $1}')"

failures=0

# Prints "<exit code> <milliseconds>"; the output goes to the log file.
run_capture() {
  local log="$1"; shift
  local start end code
  start=$(date +%s%N)
  ( "$@" ) >"$log" 2>&1
  code=$?
  end=$(date +%s%N)
  printf '%s %s' "$code" "$(( (end - start) / 1000000 ))"
}

report() { # <case> <exit> <ms> <ok?> <detail>
  if [ "$4" = 1 ]; then
    result=ok
  else
    result=MISMATCH
    failures=$((failures + 1))
  fi
  printf '%-34s %5s %8sms %-9s %s\n' "$1" "$2" "$3" "$result" "$5"
}

printf '%-34s %5s %10s %-9s %s\n' CASE EXIT TIME RESULT DETAIL

# --- 1: the real backend, the command a gate would use ----------------------

mirai_log="$work/backend-logs/mirai.log"
read -r code ms <<<"$(run_capture "$mirai_log" env \
  CARGO_TARGET_DIR="$work/backend-target" TMPDIR="$work" MIRAI_FLAGS=--diag=paranoid \
  bash -c "cd '$ws/backend' && cargo +$toolchain mirai --lib")"
ok=1
[ "$code" = 101 ] || ok=0
grep -qF 'is not supported by the following packages' "$mirai_log" || ok=0
grep -qF 'redb@' "$mirai_log" || ok=0
report 'backend:cargo-mirai --lib' "$code" "$ms" "$ok" \
  "$(grep -m1 'is not supported by the following packages' "$mirai_log" | sed 's/^error: //')"
printf '      %s\n' "$(grep -m1 -A1 'is not supported by the following packages' "$mirai_log" | tail -1 | sed 's/^ *//')"
printf '      the repository Cargo.lock is unchanged: %s\n' \
  "$([ "$repo_lock_sum" = "$(sha256sum "$here/../../Cargo.lock" | awk '{print $1}')" ] && echo yes || echo NO)"

# --- 2: every MSRV pin, to show where the workaround stops ------------------

pin_log="$work/backend-logs/pins.log"
: >"$pin_log"
accepted=0
refused=0
printf '\n'
printf '%-20s %-10s %s\n' PACKAGE PIN RESULT
for pin in "${pins[@]}"; do
  out="$( cd "$ws" && TMPDIR="$work" cargo "+$toolchain" update -p "${pin%@*}" --precise "${pin#*@}" 2>&1 )"
  printf '%s\n' "$out" >>"$pin_log"
  reason="$(printf '%s\n' "$out" | grep -m1 'failed to select a version for the requirement' | sed 's/^error: failed to select a version for the requirement //')"
  if [ -n "$reason" ]; then
    refused=$((refused + 1))
    printf '%-20s %-10s refused, %s\n' "${pin%@*}" "${pin#*@}" "$reason"
  else
    accepted=$((accepted + 1))
    printf '%-20s %-10s pinned\n' "${pin%@*}" "${pin#*@}"
  fi
done
# The last two rows are a consequence, not an independent blocker: `time 0.3.55`
# pins `time-core =0.1.9` and `time-macros =0.2.32` exactly, so those two cannot
# move while `time` is still in the graph. `time` is blocked by `cookie_store`,
# whose 0.22.1 requires `time ^0.3.47` while the newest `time` the pinned nightly
# accepts is 0.3.45. The README follows the chain to `cookie_store 0.22.0`.

check_log="$work/backend-logs/pins-check.log"
read -r code ms <<<"$(run_capture "$check_log" env TMPDIR="$work" \
  bash -c "cd '$ws' && cargo +$toolchain check --lib")"
ok=1
[ "$code" = 101 ] || ok=0
grep -qF 'is not supported by the following packages' "$check_log" || ok=0
grep -qF 'redb@' "$check_log" || ok=0
report 'backend:pinned then check --lib' "$code" "$ms" "$ok" \
  "$accepted of ${#pins[@]} pins accepted, $refused refused by a version requirement"

# --- 3: a tool failure that looks like a clean run ---------------------------

crash_log="$work/backend-logs/internal-error.log"
rm -rf "$work/fixture-internal-error" "$work/backend-target-internal-error"
cp -r "$here/fixture-internal-error" "$work/"
read -r code ms <<<"$(run_capture "$crash_log" env \
  CARGO_TARGET_DIR="$work/backend-target-internal-error" TMPDIR="$work" MIRAI_FLAGS=--diag=paranoid \
  bash -c "cd '$work/fixture-internal-error' && cargo +$toolchain mirai --lib")"
ok=1
[ "$code" = 101 ] || ok=0
grep -qF 'it does not match declaration' "$crash_log" || ok=0
# The point of the row: a crash produces no diagnostic to scrape.
[ "$(grep -c '\[MIRAI\]' "$crash_log")" = 0 ] || ok=0
report 'mirai-internal-error' "$code" "$ms" "$ok" \
  "0 [MIRAI] lines, so a scraping gate reads this as clean"

cat <<note

All three rows are expected to fail. A gate cannot be built on a tool whose
first run against this backend stops in cargo, whose fix requires changing
backend/Cargo.toml and downgrading a storage engine by two major versions, and
whose crashes are indistinguishable from a clean run by exit-code-plus-scraping
alone.
note
printf '%s row(s) did not match README.md\n' "$failures"
[ "$failures" -eq 0 ]
