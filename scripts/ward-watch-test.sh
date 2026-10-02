#!/usr/bin/env bash
# The ward watcher against stubs: no ward, no project, no network. curl, gcloud and osascript are
# stubs that answer from the environment, so each case is a set of env vars.
set -uo pipefail
TARGET="$(cd "$(dirname "$0")" && pwd)/ward-watch.sh"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bin"

cat > "$WORK/bin/curl" <<'STUB'
#!/usr/bin/env bash
for a in "$@"; do case "$a" in *hook-secret*) echo "$a" >> "$STUB_ARGV_LEAK" ;; esac; done
case "$*" in
  *api/fuel*) printf '{"relay":{"runs_left":%s}}' "${STUB_RUNS:-65000}" ;;
  *api/ward*) [ "${STUB_BOARD_DOWN:-0}" = 1 ] && exit 0
              printf '{"board":{"kept_at":%s},"queue":{"door":"open","waiting":%s}}' "${STUB_KEPT:-1000000}" "${STUB_WAITING:-20}" ;;
  *) cat > /dev/null; echo "posted" >> "$STUB_POSTS" ;;
esac
STUB
cat > "$WORK/bin/gcloud" <<'STUB'
#!/usr/bin/env bash
case "$*" in
  *"would not close"*) [ "${STUB_REFUSED:-0}" = 1 ] && echo "patient 1 is finished and would not close: x" ;;
  *"slow pass"*) echo "ward       slow pass · ${STUB_PASS:-14}.2s over 200 patients checked" ;;
  *) : ;;
esac
STUB
cat > "$WORK/bin/osascript" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$STUB_NOTES"
STUB
chmod +x "$WORK/bin/"*

PASS=0; FAIL=0
ok()  { printf '  \033[32mpass\033[0m  %s\n' "$1"; PASS=$((PASS+1)); }
bad() { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; FAIL=$((FAIL+1)); }
run() {
  local envs=(); while [ "$1" != "--" ]; do envs+=("$1"); shift; done; shift
  env -i PATH="$WORK/bin:/usr/bin:/bin" HOME="$WORK" WATCH_STATE_DIR="$WORK/state" WATCH_NOW=1000300 \
    STUB_NOTES="$WORK/notes" STUB_POSTS="$WORK/posts" STUB_ARGV_LEAK="$WORK/leak" "${envs[@]}" \
    bash "$TARGET" "$@" 2>&1
}
notes() { cat "$WORK/notes" 2>/dev/null | grep -c . ; }

echo "── ward-watch ──"
out="$(run -- production)"; rc=$?
[ "$rc" -eq 0 ] && ! printf '%s' "$out" | grep -q ALERT && [ "$(notes)" -eq 0 ] \
  && ok "a healthy ward: every check ok, exit 0, nobody notified" || bad "healthy ward misread (rc=$rc): $out"

out="$(run STUB_RUNS=1200 -- production)"; rc=$?
[ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q "relay    ALERT  1200 runs left" && [ "$(notes)" -eq 1 ] \
  && ok "a relay running dry is an alert, notified once" || bad "low relay not alerted (rc=$rc): $out"

out="$(run STUB_RUNS=1100 -- production)"
[ "$(notes)" -eq 1 ] && ok "the same alert on the next run is not notified again" || bad "a standing alert was re-notified: $(notes)"

out="$(run -- production)"
[ "$(notes)" -eq 2 ] && grep -q "all clear again" "$WORK/notes" \
  && ok "its clearing is notified once" || bad "the clearing was not said: $(cat "$WORK/notes")"

out="$(run STUB_KEPT=999000 -- production)"
printf '%s' "$out" | grep -q "board    ALERT  kept 1300s ago" && ok "a board kept over 15 minutes ago is an alert" || bad "stale board missed: $out"

out="$(run STUB_BOARD_DOWN=1 -- production)"
printf '%s' "$out" | grep -q "board    ALERT  the board could not be read" && ok "an unreadable board is an alert, not a pass" || bad "unreadable board missed: $out"

out="$(run STUB_WAITING=1 -- production)"
printf '%s' "$out" | grep -q "queue    ALERT  1 waiting" && ok "a near-empty queue with the door open is an alert" || bad "empty queue missed: $out"

out="$(run STUB_REFUSED=1 STUB_PASS=240 -- production)"
printf '%s' "$out" | grep -q "anchors  ALERT" && printf '%s' "$out" | grep -q "passes   ALERT  a pass took 240s" \
  && ok "a refused closure and a 240 s pass are both alerts" || bad "log checks missed: $out"

rm -f "$WORK/leak"; out="$(run STUB_RUNS=10 WATCH_WEBHOOK_URL=https://hooks.example/hook-secret -- staging)"
[ -s "$WORK/posts" ] && [ ! -s "$WORK/leak" ] \
  && ok "the webhook is posted to, and its URL never appears in an argument" || bad "webhook posted=$(cat "$WORK/posts" 2>/dev/null) leak=$(cat "$WORK/leak" 2>/dev/null)"

rm -f "$WORK/leak" "$WORK/posts" "$WORK/state/staging.alerts"; mkdir -p "$WORK/state"
printf 'https://hooks.example/hook-secret-from-file\n' > "$WORK/state/webhook.url"
out="$(run STUB_RUNS=10 -- staging)"; rm -f "$WORK/state/webhook.url"
[ -s "$WORK/posts" ] && [ ! -s "$WORK/leak" ] \
  && ok "a webhook kept in the state dir's file is used, and never appears in an argument" || bad "file webhook posted=$(cat "$WORK/posts" 2>/dev/null) leak=$(cat "$WORK/leak" 2>/dev/null)"

out="$(run -- prod)"; rc=$?
[ "$rc" -eq 1 ] && printf '%s' "$out" | grep -q refusing && ok "an unknown target is refused" || bad "unknown target accepted: $out"

echo; printf '%d passed, %d failed\n' "$PASS" "$FAIL"; [ "$FAIL" -eq 0 ]
