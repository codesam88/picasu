#!/usr/bin/env bash
# Partitioned inductive-invariant check for the inductive lane.
#
# quint verify --inductive-invariant cannot run the full 372-transition
# consecution in one request (two checker-side resource failures: quint
# runs the deadlock check in inductive mode, and the single-run
# full-transition-set form grows unboundedly — see
# .plan/quint-inductive-proof.md, "Resource calibration"). The protocol
# composes the same verification conditions soundly:
#
#   - `"no-deadlock": true` (consecution quantifies over arbitrary
#     I-states; deadlock detection is meaningless there),
#   - VC batches via search.invariantFilter,
#   - transition partitions via search.transitionFilter (each partition
#     ≤362 transitions; every partition rung measured flat),
#   - the composed verdict = every VC of the invariant holds over every
#     transition; each partition rung is individually recorded.
#
# Usage:
#   partitioned-typeok.sh [invariant] [port]
#
# With no arguments it checks TypeOK (the 15-domain floor). Round runs
# pass their checked invariant, e.g. Round1.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
SPEC=$HERE/../inductive-proof.qnt
LANE_DIR=${APALACHE_LANE_DIR:-$HOME/.cache/apalache-lane}
LOGDIR=$LANE_DIR/logs
INV=${1:-TypeOK}
PORT=${2:-8832}
SERVER=$HERE/apalache-server.sh

# VC batches: pairs of domain conjuncts, in the invariant's top-level
# conjunction order (VCGen flattens nested vals — the filter indexes
# top-level conjuncts of the checked invariant, 0-based). TypeOK
# flattens to 15 domain conjuncts; Round1 = TypeOK and KeyCoherent
# flattens to 19 (the four KeyCoherent conjuncts land at 15–18).
BATCHES=("0|1" "2" "4|5" "6|7" "8|9" "10|11" "12|13" "14")
case "$INV" in
  TypeOK) ;;
  Round1) BATCHES+=("15|16" "17|18") ;;
  *) echo "unknown invariant '$INV': extend BATCHES to cover its top-level conjuncts"; exit 2 ;;
esac
# Transition partitions (each ≤362; every rung measured flat)
P1='1->(0|[1-9][0-9]|1[0-7][0-9]|18[0-5])'
P2='1->(18[6-9]|19[0-9]|[12][0-9][0-9]|3[0-6][0-9]|37[0-1])'

SUM=$LOGDIR/partitioned-$INV-$(date +%Y%m%d-%H%M).tsv
: > "$SUM"

for bi in "${!BATCHES[@]}"; do
  b="${BATCHES[$bi]}"
  for pi in 1 2; do
    if [ "$pi" = 1 ]; then TF=$P1; else TF=$P2; fi
    CFG=$LANE_DIR/apalache-$(basename "$INV")-b${bi}-p${pi}.json
    printf '{\n  "checker": {\n    "no-deadlock": true,\n    "tuning": {\n      "search.invariantFilter": "1->state(%s)",\n      "search.transitionFilter": "%s"\n    }\n  }\n}\n' "$b" "$TF" > "$CFG"
    name="partitioned-$(basename "$INV")-b${bi}-vc${b//|/-}-p${pi}"
    echo "RUN $name (VCs $b, partition $pi)"
    "$SERVER" run "$name" "$SPEC" "$PORT" \
      --main inductive_proof \
      --init initCompletion --step stepCompletion \
      --invariant "$INV" --backend apalache --inductive-invariant "$INV" \
      --apalache-config "$CFG"
    rc=$?
    verdict=$(grep -oE "No violation found|Found an issue|reached a deadlock|Connection dropped|out of heap" "$LOGDIR/$name.log" | tail -1)
    echo -e "$b\t$pi\t$rc\t$verdict" >> "$SUM"
  done
done
echo "summary: $SUM"
cat "$SUM"
