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
#
# Two ownership races, found by 7b on the day it landed, are closed here:
#   - a lock with no pid file is a holder between its mkdir and publishing its pid, not a stale
#     lock: it is waited on for GATE_LOCK_GRACE seconds (default 5) before being called dead;
#   - release removes the lock only if its pid file still reads this process — a lock re-taken
#     by someone else while our command ran is theirs to release;
#   - a stale lock is broken by renaming it aside, then checking the renamed copy really holds
#     the dead pid we saw; a live pid there means the path was re-taken between our look and
#     our move, and it is given back.
set -uo pipefail

LOCK="${GATE_LOCK_DIR:-${TMPDIR:-/tmp}/vitals-gate.lock}"
WAIT="${GATE_LOCK_WAIT:-2700}"
GRACE="${GATE_LOCK_GRACE:-5}"
if [ "$#" -eq 0 ]; then
  echo "usage: scripts/gate-lock.sh <command...>" >&2; exit 2
fi

holder() { local p; p="$(cat "$1/pid" 2>/dev/null)"; [ -n "$p" ] && echo "$p" || echo "?"; }
alive()  { kill -0 "$1" 2>/dev/null; }
# Break a lock whose holder we saw dead: move it aside (one mover wins; a rename is atomic), then
# make sure what we moved is what we looked at. A live holder in the moved copy means the path
# changed hands in between — hand it back, or, if the path is already taken again, leave the
# copy where the new holder's release will sweep it.
break_lock() {
  local aside="$LOCK.stale.$$" p
  mv "$LOCK" "$aside" 2>/dev/null || return 0
  p="$(holder "$aside")"
  if [ "$p" != "?" ] && alive "$p"; then
    [ ! -e "$LOCK" ] && mv "$aside" "$LOCK" 2>/dev/null && return 0
    mv "$aside" "$LOCK/" 2>/dev/null || rm -rf "$aside"; return 0
  fi
  rm -rf "$aside"
}

waited=0; said=0; unowned=0
until mkdir "$LOCK" 2>/dev/null; do
  pid="$(holder "$LOCK")"
  if [ "$pid" = "?" ]; then
    if [ "$unowned" -ge "$GRACE" ]; then
      echo "── gate lock: no holder published a pid in ${GRACE}s — breaking an unowned lock at $LOCK" >&2
      break_lock; unowned=0; continue
    fi
    sleep 1; unowned=$((unowned + 1)); waited=$((waited + 1)); continue
  fi
  unowned=0
  if ! alive "$pid"; then
    echo "── gate lock: holder pid ${pid} is gone — breaking a stale lock at $LOCK" >&2
    break_lock; continue
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
# Publish the pid in one rename, so a reader never sees an empty pid file.
echo "$$" > "$LOCK/pid.$$" && mv "$LOCK/pid.$$" "$LOCK/pid"; date '+%Y-%m-%d %H:%M:%S' > "$LOCK/since"
release() { [ "$(cat "$LOCK/pid" 2>/dev/null)" = "$$" ] && rm -rf "$LOCK"; }
trap release EXIT INT TERM HUP
# The command can tell it is under the lock, so a script that re-execs itself through this
# wrapper does so once and not forever.
export GATE_LOCK_HELD=1
"$@"
