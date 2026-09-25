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

echo; printf '%d passed, %d failed\n' "$PASS" "$FAIL"; [ "$FAIL" -eq 0 ]
