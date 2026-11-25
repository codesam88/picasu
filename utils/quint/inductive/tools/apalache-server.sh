#!/usr/bin/env bash
# Apalache checker-server lifecycle for the inductive lane.
# Preflight-checked start, exact-PID bookkeeping via the socket table,
# disk tmpdir enforcement (never /tmp: it may be tmpfs), and a run
# wrapper. The Apalache distribution is the one quint auto-installs;
# override with APALACHE_JAR if needed.
#
# Usage:
#   apalache-server.sh start [port] [heap]
#   apalache-server.sh status [port]
#   apalache-server.sh stop [port]
#   apalache-server.sh monitor [port] [seconds]
#   apalache-server.sh run <name> <spec.qnt> [port] [quint verify args...]
set -u

LANE_DIR=${APALACHE_LANE_DIR:-$HOME/.cache/apalache-lane}
LOGDIR=$LANE_DIR/logs
DISK_TMP=$LANE_DIR/tmp
JRE_BIN="$HOME/.quint/jre/bin"
HEAP=${HEAP:-12g}
MIN_FREE_GB=${MIN_FREE_GB:-10}
CMD=$1; shift
mkdir -p "$LOGDIR" "$DISK_TMP"

discover_jar() {
  if [ -n "${APALACHE_JAR:-}" ]; then echo "$APALACHE_JAR"; return; fi
  ls "$HOME"/.quint/apalache-dist-*/apalache/lib/apalache.jar 2>/dev/null | sort | tail -1
}

mem_free_gb() { free -g | sed -n 2p | awk '{print $7}'; }
live_pid() { ss -tlnp 2>/dev/null | awk -v p=":$1" '$4 ~ p {print $NF}' | sed -n 's/.*pid=\([0-9]*\).*/\1/p' | head -1; }

preflight() {
  local fail=0
  if pgrep -f "apalache.jar" >/dev/null 2>&1; then
    echo "PREFLIGHT FAIL: apalache processes already running:"; pgrep -af "apalache.jar" | head -5; fail=1
  fi
  if pgrep -f "quint verify" >/dev/null 2>&1; then
    echo "PREFLIGHT FAIL: quint verify already running:"; pgrep -af "quint verify" | grep -v grep | head -3; fail=1
  fi
  local avail; avail=$(mem_free_gb)
  if [ "$avail" -lt "$MIN_FREE_GB" ]; then echo "PREFLIGHT FAIL: ${avail}GB free < required ${MIN_FREE_GB}GB"; fail=1; fi
  if ! df --output=fstype "$DISK_TMP" 2>/dev/null | tail -1 | grep -qE "ext[234]|xfs|btrfs|zfs"; then
    echo "PREFLIGHT FAIL: tmpdir $DISK_TMP is not on a disk filesystem (tmpfs would eat RAM)"; fail=1
  fi
  [ -x "$JRE_BIN/java" ] || { echo "PREFLIGHT FAIL: no JRE at $JRE_BIN (just install quint)"; fail=1; }
  [ -n "$(discover_jar)" ] || { echo "PREFLIGHT FAIL: no Apalache distribution under ~/.quint (run a quint verify once to install)"; fail=1; }
  return $fail
}

case "$CMD" in
  start)
    PORT=${1:-8832}; HEAP=${2:-$HEAP}
    preflight || exit 1
    if ss -tln 2>/dev/null | grep -q ":$PORT "; then echo "FAIL: port $PORT in use"; exit 1; fi
    JAR=$(discover_jar)
    export JRE_BIN PATH="$JRE_BIN:$PATH" TMPDIR="$DISK_TMP"
    export JAVA_TOOL_OPTIONS="-Djava.io.tmpdir=$DISK_TMP"
    nohup java -Xmx$HEAP -Xms2g -XX:+UseG1GC -Djava.io.tmpdir="$DISK_TMP" \
      -jar "$JAR" server --port="$PORT" > "$LOGDIR/server-$PORT.log" 2>&1 &
    sleep 15
    local_pid=$(live_pid "$PORT")
    if [ -z "$local_pid" ] || ! grep -q java "/proc/$local_pid/comm" 2>/dev/null; then
      echo "START FAIL: no java listening on $PORT after 15 s"; tail -5 "$LOGDIR/server-$PORT.log"; exit 1
    fi
    echo "STARTED pid=$local_pid port=$PORT heap=$HEAP free=$(mem_free_gb)GB"
    ;;
  status)
    PORT=${1:-8832}; jpid=$(live_pid "$PORT")
    if [ -n "$jpid" ]; then
      echo "ALIVE port=$PORT pid=$jpid rss_kb=$(ps -o rss= -p "$jpid" | tr -d ' ') free=$(mem_free_gb)GB"
    else
      echo "DEAD port=$PORT"
    fi
    ;;
  stop)
    PORT=${1:-8832}; jpid=$(live_pid "$PORT")
    if [ -n "$jpid" ]; then kill "$jpid" && echo "stopped pid=$jpid"; else echo "nothing on $PORT"; fi
    ;;
  monitor)
    PORT=${1:-8832}; dur=${2:-600}; jpid=$(live_pid "$PORT")
    [ -n "$jpid" ] || { echo "no server on $PORT"; exit 1; }
    f="$LOGDIR/mem-$PORT.log"; : > "$f"
    for i in $(seq 1 $((dur / 5))); do
      r=$(ps -o rss= -p "$jpid" 2>/dev/null | tr -d ' ')
      echo "$(date +%H:%M:%S) rss_kb=$r free_gb=$(mem_free_gb)" >> "$f"
      [ -z "$r" ] && break
      sleep 5
    done
    echo "monitor written: $f"
    ;;
  run)
    NAME=$1; SPEC=$2; PORT=$3; shift 3
    jpid=$(live_pid "$PORT")
    if [ -z "$jpid" ]; then echo "no server on $PORT — start one first"; exit 1; fi
    export JRE_BIN PATH="$JRE_BIN:$PATH" TMPDIR="$DISK_TMP"
    export JAVA_TOOL_OPTIONS="-Djava.io.tmpdir=$DISK_TMP"
    f="$LOGDIR/$NAME.log"
    timeout ${RUN_CAP:-3600} quint verify "$SPEC" "$@" --server-endpoint "localhost:$PORT" > "$f" 2>&1
    rc=$?; echo "exit:$rc" >> "$f"
    echo "RUN $NAME rc=$rc log=$f"
    grep -E "NoError|No violation found|Found an issue|reached a deadlock|state invariant [0-9]+ holds" "$f" | tail -5
    ;;
  *) echo "usage: $0 {start|status|stop|monitor|run} ..."; exit 2 ;;
esac
