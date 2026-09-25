#!/usr/bin/env bash
# Run one command under the machine-wide gate lock, so two gate runs on this Mac serialize
# instead of racing.
#
#   scripts/gate-lock.sh <command...>
#
# Why. On 25 Sep 2026 two sessions ran the workspace tests at once on the mini — different
# worktrees, so no shared target dir, but the CPU contention alone turned one run RED with no
# failing test to show for it; a solo re-run was green. Contention fabricates failures, never
# passes, so the red cost a re-run and a diagnosis. A pre-flight `pgrep cargo` on one side
# stopped that side from starting on top of the other and could not stop the other from starting
# on top of it: two pre-flights still race. The only thing that cannot race is a lock in the
# script both sides run — this one — held for the whole run, released when the command exits,
# however it exits.
#
# The lock is a directory (mkdir is atomic on every filesystem this runs on, and needs no flock
# binary, which macOS does not ship). It holds the pid and start time of the holder. A lock whose
# pid is dead is stale — a killed session — and is broken rather than waited on forever; a lock
# whose pid is alive is waited on, with a line every thirty seconds saying whose it is, up to
# GATE_LOCK_WAIT seconds (default 45 minutes, longer than any gate run) and then a refusal.
set -uo pipefail

LOCK="${GATE_LOCK_DIR:-${TMPDIR:-/tmp}/vitals-gate.lock}"
WAIT="${GATE_LOCK_WAIT:-2700}"
if [ "$#" -eq 0 ]; then
  echo "usage: scripts/gate-lock.sh <command...>" >&2; exit 2
fi

holder() { cat "$LOCK/pid" 2>/dev/null || echo "?"; }
alive()  { kill -0 "$1" 2>/dev/null; }

waited=0; said=0
until mkdir "$LOCK" 2>/dev/null; do
  pid="$(holder)"
  if [ "$pid" = "?" ] || ! alive "$pid"; then
    echo "── gate lock: holder pid ${pid} is gone — breaking a stale lock at $LOCK" >&2
    rm -rf "$LOCK"; continue
  fi
  if [ "$waited" -ge "$WAIT" ]; then
    echo "refusing: the gate lock at $LOCK has been held by pid $pid for ${WAIT}s — another gate run is still going. Wait for it, or if it is dead, remove the directory." >&2
    exit 3
  fi
  if [ $((waited - said)) -ge 30 ] || [ "$waited" -eq 0 ]; then
    echo "── gate lock: waiting on pid $pid ($(cat "$LOCK/since" 2>/dev/null || echo '?')) — ${waited}s" >&2; said=$waited
  fi
  sleep 2; waited=$((waited + 2))
done
echo "$$" > "$LOCK/pid"; date '+%Y-%m-%d %H:%M:%S' > "$LOCK/since"
trap 'rm -rf "$LOCK"' EXIT INT TERM HUP
# The command can tell it is under the lock, so a script that re-execs itself through this
# wrapper does so once and not forever.
export GATE_LOCK_HELD=1
"$@"
