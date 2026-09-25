#!/usr/bin/env bash
# Prove scripts/gate-lock.sh serializes two gate runs and breaks a stale lock — without cargo.
#
#   scripts/gate-lock-test.sh
set -uo pipefail
cd "$(dirname "$0")/.."
L="$PWD/target/gate-lock-test.lock"; rm -rf "$L"; mkdir -p target
PASS=0; FAIL=0
ok()  { printf '  \033[32mpass\033[0m  %s\n' "$1"; PASS=$((PASS+1)); }
bad() { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; FAIL=$((FAIL+1)); }

echo "── gate-lock ──"
# 1. two runs of a 2 s command must not overlap: the pair takes at least 4 s, not 2.
t0=$(date +%s)
GATE_LOCK_DIR="$L" bash scripts/gate-lock.sh sleep 2 & a=$!
sleep 0.3
GATE_LOCK_DIR="$L" bash scripts/gate-lock.sh sleep 2 & b=$!
wait $a; wait $b; dt=$(( $(date +%s) - t0 ))
[ "$dt" -ge 4 ] && ok "two concurrent gate runs serialize (pair took ${dt}s, not ~2)" || bad "two gate runs overlapped (pair took ${dt}s)"

# 2. the lock is gone after the command exits, whatever it exits with.
GATE_LOCK_DIR="$L" bash scripts/gate-lock.sh false; rc=$?
[ ! -d "$L" ] && [ "$rc" -eq 1 ] && ok "lock released after a failing command, and its exit code kept ($rc)" || bad "lock left behind or exit code lost (rc=$rc, dir present=$([ -d "$L" ] && echo yes || echo no))"

# 3. a stale lock — holder pid dead — is broken, not waited on.
mkdir -p "$L"; echo 999999 > "$L/pid"; echo "2026-09-25 00:00:00" > "$L/since"
t0=$(date +%s); out="$(GATE_LOCK_DIR="$L" bash scripts/gate-lock.sh echo ran 2>&1)"; dt=$(( $(date +%s) - t0 ))
printf '%s' "$out" | grep -q 'breaking a stale lock' && printf '%s' "$out" | grep -q '^ran' && [ "$dt" -lt 10 ] \
  && ok "a stale lock (dead pid) is broken and the command runs (${dt}s)" || bad "stale lock not broken: $out"

# 4. a live lock is waited on, and the wait cap refuses rather than hanging.
mkdir -p "$L"; echo $$ > "$L/pid"; echo "now" > "$L/since"
out="$(GATE_LOCK_WAIT=3 GATE_LOCK_DIR="$L" bash scripts/gate-lock.sh echo ran 2>&1)"; rc=$?
[ "$rc" -eq 3 ] && printf '%s' "$out" | grep -q 'refusing' && ! printf '%s' "$out" | grep -q '^ran' \
  && ok "a live lock is waited on and the cap refuses (exit 3), never runs the command" || bad "live lock not respected (rc=$rc): $out"
rm -rf "$L"

# 5. a lock whose holder has not yet published its pid is a holder mid-publication, not a stale
#    lock: it is waited on. Here the pid appears after a second and is alive, so the capped wait
#    must refuse — never break the lock and run.
mkdir -p "$L"
( sleep 1; echo $$ > "$L/pid"; echo now > "$L/since" ) &
out="$(GATE_LOCK_WAIT=3 GATE_LOCK_GRACE=5 GATE_LOCK_DIR="$L" bash scripts/gate-lock.sh echo ran 2>&1)"; rc=$?
wait
[ "$rc" -eq 3 ] && ! printf '%s' "$out" | grep -q '^ran' && ! printf '%s' "$out" | grep -q 'breaking' \
  && ok "a lock without a pid yet is waited on, not broken (holder published 1 s later; rc=$rc)" || bad "a lock mid-publication was broken (rc=$rc): $out"
rm -rf "$L"

# 6. a lock that stays without a pid past the grace is a holder that died between mkdir and
#    publishing — broken, and the command runs.
mkdir -p "$L"
t0=$(date +%s); out="$(GATE_LOCK_GRACE=2 GATE_LOCK_DIR="$L" bash scripts/gate-lock.sh echo ran 2>&1)"; dt=$(( $(date +%s) - t0 ))
printf '%s' "$out" | grep -q 'breaking an unowned lock' && printf '%s' "$out" | grep -q '^ran' && [ "$dt" -lt 10 ] \
  && ok "a lock with no pid past the grace is broken and the command runs (${dt}s)" || bad "unowned lock not broken after grace (${dt}s): $out"
rm -rf "$L"

# 7. release removes only a lock this process still owns: if the path was re-taken by another
#    holder while the command ran, the exit leaves that holder's lock alone.
GATE_LOCK_DIR="$L" bash scripts/gate-lock.sh bash -c "echo 999999 > '$L/pid'"
[ -d "$L" ] && [ "$(cat "$L/pid" 2>/dev/null)" = 999999 ] \
  && ok "release leaves a lock that another holder re-took (pid file now reads theirs)" || bad "release removed a lock it no longer owned (dir present=$([ -d "$L" ] && echo yes || echo no))"
rm -rf "$L"

# 8. a stale break leaves no debris beside the lock path.
mkdir -p "$L"; echo 999999 > "$L/pid"
GATE_LOCK_DIR="$L" bash scripts/gate-lock.sh true >/dev/null 2>&1
[ -z "$(ls -d "$L".stale.* 2>/dev/null)" ] && [ ! -d "$L" ] && ok "a stale break leaves nothing behind" || bad "stale break left debris: $(ls -d "$L"* 2>/dev/null | tr '\n' ' ')"
rm -rf "$L" "$L".stale.* 2>/dev/null

echo; printf '%d passed, %d failed\n' "$PASS" "$FAIL"; [ "$FAIL" -eq 0 ]
