#!/usr/bin/env bash
# After a push through scripts/push-case.sh: prove from the chain that no case it replaced was
# carrying an anchored shift at the moment it was replaced.
#
#   scripts/push-case-verify.sh <staging|production> <push log>
#
# Why. push-case.sh reads the board before it sends, and the door reads the board before it
# stores; neither can see a shift anchored after that board was kept. The residual risk on every
# push is that window, not the number of readers (7b, 23 Sep 2026). This asks the chain, after
# the fact: for every patient on a case the log says was stored, every anchored shift's slot →
# its block time → before or after the push. A shift anchored before the push finished means
# that case was pinned when it was replaced and its leaf no longer re-derives — the failure of
# 14:44 the same day, quietly, on a case nobody flagged; the repair is the deterministic
# recompile at the compiler commit the previous pack names. The push window is the log's own
# time span: its first line's clock, if it has one, else fifteen minutes before its last write.
set -uo pipefail

TARGET="${1:-}"; LOG="${2:-}"
case "$TARGET" in
  staging)    WARD="${VITALS_WARD_URL:-https://vitals-world-367117259093.asia-southeast1.run.app}" ;;
  production) WARD="${VITALS_WARD_URL:-https://world.vitals.academy}" ;;
  *) echo "refusing: the target is 'staging' or 'production', not '${TARGET:-nothing}'." >&2
     echo "    scripts/push-case-verify.sh <staging|production> <push log>" >&2; exit 1 ;;
esac
if [ ! -f "$LOG" ]; then echo "refusing: '$LOG' is not a push log." >&2; exit 1; fi
RPC="${VITALS_RPC:-https://api.devnet.solana.com}"
END="$(stat -f '%m' "$LOG" 2>/dev/null || stat -c '%Y' "$LOG")"

BOARD="$(curl -sS -m 90 "$WARD/api/ward" 2>/dev/null)" || BOARD=""
if [ -z "$BOARD" ]; then echo "cannot tell: $WARD/api/ward did not answer — an unknown is not a no." >&2; exit 2; fi

python3 - "$LOG" "$BOARD" "$END" "$WARD" "$RPC" <<'PY'
import json, sys, time, datetime, urllib.request
log, board, end, ward, rpc = sys.argv[1], json.loads(sys.argv[2]), int(sys.argv[3]), sys.argv[4], sys.argv[5]
stored = [l.split()[1] for l in open(log, encoding="utf-8") if l.strip().startswith("stored:") and "unchanged:" not in l]
def case_of(p):
    c = p.get("case"); return c if isinstance(c, str) else (c or {}).get("case_id")
def call(method, params):
    req = urllib.request.Request(rpc, data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(), headers={"content-type": "application/json"})
    return json.load(urllib.request.urlopen(req, timeout=30)).get("result")
ict = datetime.timezone(datetime.timedelta(hours=7))
before, after, unknown, live = [], [], [], []
for p in board.get("patients", []):
    cid = case_of(p)
    if cid not in stored:
        continue
    slots = []
    if p.get("closed_slot"):
        slots.append(("closure", p["closed_slot"]))
    sh = p.get("shifts"); n = len(sh) if isinstance(sh, list) else int(sh or 0)
    if n > 0:
        try:
            d = json.load(urllib.request.urlopen(f"{ward}/api/ward/patient/{p['patient_id']}", timeout=60))
            for s in d.get("shifts") or []:
                if s.get("slot") and ("closure", s["slot"]) not in slots:
                    slots.append(("shift", s["slot"]))
        except Exception as e:
            unknown.append((cid, p.get("patient_id"), f"patient route: {e}"))
    if p.get("on_shift_since"):
        live.append((cid, p.get("patient_id"), p["on_shift_since"]))
    for kind, slot in slots:
        t = call("getBlockTime", [slot]); time.sleep(0.2)
        if t is None:
            unknown.append((cid, p.get("patient_id"), f"{kind} slot {slot}: no block time")); continue
        when = datetime.datetime.fromtimestamp(t, ict).strftime("%d %b %H:%M:%S ICT")
        (before if t < end else after).append((cid, p.get("patient_id"), kind, slot, when))
examined = len(before) + len(after) + len(unknown)
print(f"── {len(stored)} case(s) the log says were stored · board {board.get('revision')} · push finished {datetime.datetime.fromtimestamp(end, ict).strftime('%d %b %H:%M:%S ICT')} · {examined} anchored shift(s) examined on those cases, {len(live)} in progress")
print(f"── anchored before the push finished — pinned when replaced: {len(before)}")
for r in before: print("   ", *r)
print(f"── anchored after the push — played on the new bytes: {len(after)}")
for r in after: print("   ", *r)
print(f"── in progress now: {len(live)}")
for r in live: print("   ", *r)
if unknown:
    print(f"── could not tell: {len(unknown)}")
    for r in unknown: print("   ", *r)
if before:
    print("FAIL: a case was replaced under an anchored shift — restore it from the compiler commit its previous pack names, then decide."); sys.exit(1)
if unknown:
    print("cannot tell for every shift — an unknown is not a no."); sys.exit(2)
print("clean: no replaced case carried a shift at the moment it was replaced.")
PY
