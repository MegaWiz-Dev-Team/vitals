#!/usr/bin/env bash
# Read the ward's integrity list — GET /api/ward/bytes — the way it is meant to be read since
# a5f609a: as pages. Each call has a five-second budget and stops between patients; its counts are
# the page's, and the ward's totals are the sum over a complete paged read. This script does the
# summing, logs the wall time of every call (the thing that held the instance for 140 s before the
# budget existed), and prints one line of totals with how they were reached.
#
#   scripts/bytes-read.sh <staging|production>
#
# Runs as the ops service account by construction (configuration pinned per process, never
# `gcloud config set`) and reads the door token into this shell and nowhere else. Read-only: it
# never seeds, never takes a shift, and is safe on production.
set -uo pipefail

TARGET="${1:-}"
case "$TARGET" in
  staging)
    PROJECT=vitals-academy-dev; CONFIG=vitals-ops-dev
    WARD="${VITALS_WARD_URL:-https://vitals-world-367117259093.asia-southeast1.run.app}" ;;
  production)
    PROJECT=vitals-academy; CONFIG=vitals-ops
    WARD="${VITALS_WARD_URL:-https://world.vitals.academy}" ;;
  *)
    echo "refusing: the target is 'staging' or 'production', not '${TARGET:-nothing}'." >&2
    echo "    scripts/bytes-read.sh <staging|production>" >&2
    exit 1 ;;
esac
export CLOUDSDK_ACTIVE_CONFIG_NAME="$CONFIG"
TOKEN="$(gcloud secrets versions access latest --secret vitals-door-token --project "$PROJECT" 2>/dev/null)"
if [ -z "$TOKEN" ]; then
  echo "refusing: the door token could not be read under configuration $CONFIG. Nothing was asked." >&2
  exit 1
fi

MAX_CALLS="${BYTES_MAX_CALLS:-200}"
# Plain variables, not an associative array: the system bash on macOS is 3.2 and CI's may be any.
after=""; calls=0; slowest=0
s_shifts=0; s_proved=0; s_proved_as_it_stands=0; s_ambiguous=0; s_unrebuildable=0; s_no_tape=0; s_not_asked=0; s_disagreements=0
echo "── $TARGET · $WARD/api/ward/bytes · $(date '+%Y-%m-%d %H:%M:%S %Z')"
while :; do
  calls=$((calls + 1))
  if [ "$calls" -gt "$MAX_CALLS" ]; then
    echo "stopping: $MAX_CALLS calls and the ward still says incomplete — the totals below are partial." >&2
    break
  fi
  q=""; [ -n "$after" ] && q="?after=$after"
  t0=$(python3 -c 'import time; print(time.time())')
  ANSWER="$(curl -sS -m 300 -H "authorization: Bearer $TOKEN" -w '\n%{http_code}' "$WARD/api/ward/bytes$q" 2>/dev/null)"
  t1=$(python3 -c 'import time; print(time.time())')
  CODE="${ANSWER##*$'\n'}"; BODY="${ANSWER%$'\n'*}"
  wall="$(python3 -c "print(round($t1 - $t0, 1))")"
  if [ "$CODE" != 200 ]; then
    echo "call $calls: the ward answered $CODE after ${wall}s — stopping; totals below are partial." >&2
    echo "${BODY:0:300}" >&2; break
  fi
  line="$(printf '%s' "$BODY" | python3 -c '
import json,sys
b=json.load(sys.stdin)
keys=["shifts","proved","proved_as_it_stands","ambiguous","unrebuildable","no_tape","not_asked","disagreements"]
print(" ".join(f"{k}={int(b.get(k,0))}" for k in keys), "complete=%s" % ("yes" if b.get("complete") else "no"), "next_after=%s" % (b.get("next_after") if b.get("next_after") is not None else "-"), "read_from=%s" % (b.get("read_from") if b.get("read_from") is not None else "-"))
' 2>/dev/null)"
  if [ -z "$line" ]; then
    echo "call $calls: the answer was not the page this script knows — stopping; totals below are partial." >&2
    echo "${BODY:0:300}" >&2; break
  fi
  echo "call $calls: ${wall}s  $line"
  for kv in $line; do
    k="${kv%%=*}"; v="${kv#*=}"
    case "$k" in
      shifts)              s_shifts=$((s_shifts + v)) ;;
      proved)              s_proved=$((s_proved + v)) ;;
      proved_as_it_stands) s_proved_as_it_stands=$((s_proved_as_it_stands + v)) ;;
      ambiguous)           s_ambiguous=$((s_ambiguous + v)) ;;
      unrebuildable)       s_unrebuildable=$((s_unrebuildable + v)) ;;
      no_tape)             s_no_tape=$((s_no_tape + v)) ;;
      not_asked)           s_not_asked=$((s_not_asked + v)) ;;
      disagreements)       s_disagreements=$((s_disagreements + v)) ;;
    esac
  done
  slowest="$(python3 -c "print(max($slowest, $wall))")"
  complete="$(printf '%s' "$line" | sed -n 's/.*complete=\([a-z]*\).*/\1/p')"
  next="$(printf '%s' "$line" | sed -n 's/.*next_after=\([^ ]*\).*/\1/p')"
  if [ "$complete" = yes ] || [ "$next" = "-" ]; then break; fi
  after="$next"
done
echo "── totals over $calls call(s), slowest ${slowest}s — summed by this script from the pages, each page's counts being that page's only:"
printf '   shifts %d · proved %d · proved as it stands %d · ambiguous %d · unrebuildable %d · no tape %d · not asked %d · disagreements %d\n' \
  "$s_shifts" "$s_proved" "$s_proved_as_it_stands" "$s_ambiguous" "$s_unrebuildable" "$s_no_tape" "$s_not_asked" "$s_disagreements"
if [ "$s_shifts" -gt 0 ]; then
  python3 -c "print('   no tape as a share of every shift on the list: %d/%d = %d percent' % ($s_no_tape, $s_shifts, round(100*$s_no_tape/$s_shifts)))"
fi
[ "${complete:-no}" = yes ]
