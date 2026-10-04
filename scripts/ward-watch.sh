#!/usr/bin/env bash
# Watch the ward and say when something that matters changes — pillar 4 of the mainnet plan.
#
#   scripts/ward-watch.sh <staging|production>
#
# Every check is a public GET or a log read as the ops service account; nothing here writes to the
# ward, takes a shift or reads a secret. One line per check, `ok` or `ALERT`, and the exit status is
# the number of alerts (capped at 1). Meant to run every ten minutes (launchd), so it notifies only
# when the set of alerts *changes* — a stuck alert is said once, and its clearing is said once too.
#
# The checks, and why each is one:
#   relay      the relay pays every admission and anchor; empty, and nobody can hand over (devnet
#              play money now, real money on mainnet). Alert under WATCH_MIN_DAYS days of ward left
#              (default 14) — or, from a server that reports no days, WATCH_MIN_RUNS runs (5000).
#   board      the pass re-keeps the board; a board older than WATCH_BOARD_MAX_SECS (default 900)
#              means the ward stopped moving — 28 Sep 2026 it sat 12+ minutes old while passes died.
#   queue      the patient factory keeps the queue full; fewer than WATCH_MIN_QUEUE waiting (default 3)
#              with the door open means it stopped making patients.
#   anchors    "would not close" in the last WATCH_WINDOW_MINS (default 30) of logs: a closure the
#              chain refused.
#   passes     a pass over WATCH_PASS_MAX_SECS (default 120) in the window.
#   memory     "Memory limit" in the window — the 28 Sep kill loop.
#   errors     more than WATCH_MAX_5XX (default 10) 5xx answers in the window.
#
# Notification: a macOS notification always, and a POST of the summary to $WATCH_WEBHOOK_URL when
# that is set (a Discord or Slack incoming-webhook URL — kept out of this repo and out of argv).
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
    echo "    scripts/ward-watch.sh <staging|production>" >&2
    exit 1 ;;
esac
export CLOUDSDK_ACTIVE_CONFIG_NAME="$CONFIG"

MIN_RUNS="${WATCH_MIN_RUNS:-5000}"
MIN_DAYS="${WATCH_MIN_DAYS:-14}"
BOARD_MAX="${WATCH_BOARD_MAX_SECS:-900}"
MIN_QUEUE="${WATCH_MIN_QUEUE:-3}"
WINDOW="${WATCH_WINDOW_MINS:-30}"
PASS_MAX="${WATCH_PASS_MAX_SECS:-120}"
MAX_5XX="${WATCH_MAX_5XX:-10}"
STATE_DIR="${WATCH_STATE_DIR:-$HOME/.vitals/watch}"
# The webhook (the Asgard Discord channel, 2 Oct 2026) lives in a file only its owner can read, so
# it is never in the launchd plist, the repo, or an argument. The env var still wins when set.
WEBHOOK_FILE="${WATCH_WEBHOOK_FILE:-$STATE_DIR/webhook.url}"
if [ -z "${WATCH_WEBHOOK_URL:-}" ] && [ -r "$WEBHOOK_FILE" ]; then
  WATCH_WEBHOOK_URL="$(head -1 "$WEBHOOK_FILE" | tr -d '[:space:]')"
fi
NOW="${WATCH_NOW:-$(date +%s)}"

ALERTS=()
say() { printf '%-8s %-6s %s\n' "$1" "$2" "$3"; [ "$2" = ALERT ] && ALERTS+=("$1: $3"); }

# ── the ward's own public answers ────────────────────────────────────────────
FUEL="$(curl -s -m 60 "$WARD/api/fuel" 2>/dev/null)"
# Days at the ward's own pace when the ward reports them (4 Oct 2026): runs_left divides by a
# player's run and never counted the admissions, ~99 % of what the ward's relay spends, so it said
# ~50× too much and this alert would have fired after the relay was already empty.
read -r DAYS RUNS <<<"$(printf '%s' "$FUEL" | python3 -c 'import json,sys
try:
    r=json.load(sys.stdin)["relay"]; w=r.get("ward_runway") or {}
    print(w.get("days_left","-"), r.get("runs_left","-"))
except Exception: print("- -")' 2>/dev/null)"
if [ "$DAYS" != "-" ] && [ -n "$DAYS" ]; then
  if [ "$DAYS" -lt "$MIN_DAYS" ]; then say relay ALERT "$DAYS days of ward left (under $MIN_DAYS) — refill the relay"
  else say relay ok "$DAYS days of ward left"; fi
elif [ "$RUNS" = "-" ] || [ -z "$RUNS" ]; then say relay ALERT "the relay balance could not be read from /api/fuel"
elif [ "$RUNS" -lt "$MIN_RUNS" ]; then say relay ALERT "$RUNS runs left (under $MIN_RUNS) — refill the relay"
else say relay ok "$RUNS runs left"; fi

BOARD="$(curl -s -m 60 "$WARD/api/ward" 2>/dev/null)"
read -r KEPT DOOR WAITING <<<"$(printf '%s' "$BOARD" | python3 -c 'import json,sys
try:
    d=json.load(sys.stdin); q=d.get("queue") or {}
    print((d.get("board") or {}).get("kept_at") or "-", q.get("door") or "-", q.get("waiting") if q.get("waiting") is not None else "-")
except Exception: print("- - -")' 2>/dev/null)"
if [ "$KEPT" = "-" ]; then say board ALERT "the board could not be read from /api/ward"
else
  AGE=$(( NOW - KEPT ))
  if [ "$AGE" -gt "$BOARD_MAX" ]; then say board ALERT "kept ${AGE}s ago (over ${BOARD_MAX}s) — the ward's pass may have stopped"
  else say board ok "kept ${AGE}s ago"; fi
fi
if [ "$DOOR" = open ] && [ "$WAITING" != "-" ] && [ "$WAITING" -lt "$MIN_QUEUE" ]; then
  say queue ALERT "$WAITING waiting with the door open — the patient factory may have stopped"
else say queue ok "door $DOOR · $WAITING waiting"; fi

# ── the logs, over the window ────────────────────────────────────────────────
SINCE="$(python3 -c "import datetime,sys;print(datetime.datetime.fromtimestamp($NOW-$WINDOW*60,datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))")"
logs() {
  gcloud logging read "resource.labels.service_name=\"vitals-world\" AND timestamp>=\"$SINCE\" AND $1" \
    --project "$PROJECT" --limit 500 --format='value(textPayload)' 2>/dev/null
}
N="$(logs 'textPayload:"would not close"' | grep -c . )"
if [ "$N" -gt 0 ]; then say anchors ALERT "$N closure(s) the chain refused in the last ${WINDOW} min"
else say anchors ok "no refused closure in ${WINDOW} min"; fi

WORST="$(logs 'textPayload:"slow pass"' | sed -nE 's/.*slow pass · ([0-9]+)\.[0-9]+s.*/\1/p' | sort -n | tail -1)"
if [ -n "$WORST" ] && [ "$WORST" -gt "$PASS_MAX" ]; then say passes ALERT "a pass took ${WORST}s (over ${PASS_MAX}s)"
else say passes ok "slowest pass ${WORST:-under 10}s"; fi

M="$(logs 'textPayload:"Memory limit"' | grep -c . )"
if [ "$M" -gt 0 ]; then say memory ALERT "killed for memory $M time(s) in ${WINDOW} min"
else say memory ok "no memory kill"; fi

E="$(gcloud logging read "resource.labels.service_name=\"vitals-world\" AND timestamp>=\"$SINCE\" AND httpRequest.status>=500" \
      --project "$PROJECT" --limit 500 --format='value(httpRequest.status)' 2>/dev/null | grep -c . )"
if [ "$E" -gt "$MAX_5XX" ]; then say errors ALERT "$E server errors in ${WINDOW} min"
else say errors ok "$E server errors in ${WINDOW} min"; fi

# ── say it once ──────────────────────────────────────────────────────────────
mkdir -p "$STATE_DIR"
STATE="$STATE_DIR/$TARGET.alerts"
# Numbers are dropped before comparing, so "1200 runs left" and "1100 runs left" are one standing
# alert rather than a new one every ten minutes. -E, because BSD sed has no \+.
NEWSTATE="$(printf '%s\n' "${ALERTS[@]:-}" | sed -E 's/[0-9]+/N/g' | sort)"
OLDSTATE="$(cat "$STATE" 2>/dev/null)"
if [ "$NEWSTATE" != "$OLDSTATE" ]; then
  printf '%s' "$NEWSTATE" > "$STATE"
  if [ "${#ALERTS[@]}" -gt 0 ]; then MSG="Vitals $TARGET: ${ALERTS[*]}"; else MSG="Vitals $TARGET: all clear again"; fi
  echo "notify   $MSG"
  command -v osascript >/dev/null && osascript -e "display notification \"${MSG//\"/\'}\" with title \"Vitals ward watch\"" 2>/dev/null
  if [ -n "${WATCH_WEBHOOK_URL:-}" ]; then
    python3 -c 'import json,sys;print(json.dumps({"content":sys.argv[1],"text":sys.argv[1]}))' "$MSG" \
      | curl -s -m 20 -X POST -H 'content-type: application/json' --data-binary @- \
        -K <(printf 'url = "%s"\n' "$WATCH_WEBHOOK_URL") >/dev/null
  fi
fi
[ "${#ALERTS[@]}" -eq 0 ]
