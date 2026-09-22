#!/usr/bin/env bash
# Every call a deploy day needs, made as the ops service account, one line each, days before.
#
#   scripts/opening-check.sh <staging|production>
#   EXPECT_MIN=0 scripts/opening-check.sh production     (also check min-instances against a ruling)
#
# Why. On 21 Sep 2026 the service account could read the secrets and describe the service, and the
# ward still opened two and a half hours late: the one call nobody had tried as that account was
# the image pull the evening needed, and the founder's login — the fallback — had expired at 18:52.
# A check that is run the day before, as the identity that will act, is the only kind that would
# have caught it. Each line here is a call the deploy script, the door script, the ticker or the
# factory actually makes; "every check passed" means every one of them was made and answered.
#
# Nothing here changes anything: the one write is a one-byte probe object in the Cloud Build
# bucket, removed on the spot. Secrets are read for length only and never printed.
set -uo pipefail

TARGET="${1:-}"
case "$TARGET" in
  staging)
    PROJECT=vitals-academy-dev; CONFIG=vitals-ops-dev
    WARD="${VITALS_WARD_URL:-https://vitals-world-367117259093.asia-southeast1.run.app}"
    TICK_EXPECTED=0 ;;
  production)
    PROJECT=vitals-academy; CONFIG=vitals-ops
    WARD="${VITALS_WARD_URL:-https://world.vitals.academy}"
    TICK_EXPECTED=1 ;;
  *)
    echo "refusing: the target is 'staging' or 'production', not '${TARGET:-nothing}'." >&2
    echo "    scripts/opening-check.sh <staging|production>" >&2
    exit 1 ;;
esac
SERVICE=vitals-world; REGION=asia-southeast1
EXPECT_ACCOUNT="vitals-ops@${PROJECT}.iam.gserviceaccount.com"
FACTORY_LABEL=com.vitals.world-factory-prod
FACTORY_LOG="$HOME/.vitals/world-prod/factory.log"

FAILED=0
# Colour only for a person at a terminal; a log, a harness or a message reads the plain words.
if [ -t 1 ]; then C_OK=$'\033[32m'; C_SKIP=$'\033[33m'; C_BAD=$'\033[31m'; C_END=$'\033[0m'; else C_OK=""; C_SKIP=""; C_BAD=""; C_END=""; fi
ok()   { printf '  %sok%s    %s\n' "$C_OK" "$C_END" "$1"; }
skip() { printf '  %sskip%s  %s\n' "$C_SKIP" "$C_END" "$1"; }
bad()  { printf '  %sFAIL%s  %s\n' "$C_BAD" "$C_END" "$1"; FAILED=$((FAILED + 1)); }
jsonq() { python3 -c "import json,sys; b=json.load(sys.stdin); print($1)" 2>/dev/null; }

echo "── opening check · $TARGET · $(date -u '+%Y-%m-%d %H:%M UTC')"

# Identity first: everything below is only meaningful as the account that will act on the day.
export CLOUDSDK_ACTIVE_CONFIG_NAME="$CONFIG"
WHO="$(gcloud config get-value account 2>/dev/null)"
if [ "$WHO" != "$EXPECT_ACCOUNT" ]; then
  echo "refusing: '${WHO:-nobody}' is a person, or the wrong account — this check runs as $EXPECT_ACCOUNT" >&2
  echo "(configuration '$CONFIG'), because that is who acts on the day." >&2
  exit 1
fi
ok "account $WHO"
if ! gcloud auth print-access-token >/dev/null 2>&1; then
  echo "refusing: no usable credential for $WHO — the key is missing or revoked." >&2
  exit 1
fi
ok "credential"

# The two tokens the ward is deployed with and the door script reads. Length only.
for S in vitals-door-token vitals-token; do
  N="$(gcloud secrets versions access latest --secret "$S" --project "$PROJECT" 2>/dev/null | wc -c | tr -d ' ')"
  if [ "${N:-0}" -gt 0 ]; then ok "secret $S readable"; else bad "secret $S — cannot be read as $WHO"; fi
done

# The service as it is: revision, door, min-instances, RPC.
SVC="$(gcloud run services describe "$SERVICE" --project "$PROJECT" --region "$REGION" --format=json 2>/dev/null)"
if [ -z "$SVC" ]; then
  bad "service $SERVICE in $PROJECT cannot be described"
  REV=""; DOOR=""; MIN=""; RPC=""
else
  REV="$(printf '%s' "$SVC" | jsonq 'b["status"].get("latestReadyRevisionName","")')"
  DOOR="$(printf '%s' "$SVC" | jsonq 'next((e.get("value","") for e in b["spec"]["template"]["spec"]["containers"][0].get("env",[]) if e.get("name")=="VITALS_WARD_DOOR"),"closed")')"
  MIN="$(printf '%s' "$SVC" | jsonq 'b["spec"]["template"].get("metadata",{}).get("annotations",{}).get("autoscaling.knative.dev/minScale","0")')"
  RPC="$(printf '%s' "$SVC" | jsonq 'next((e.get("value","") for e in b["spec"]["template"]["spec"]["containers"][0].get("env",[]) if e.get("name")=="VITALS_RPC"),"https://api.devnet.solana.com")')"
  ok "service $SERVICE serving $REV · door $DOOR"
  if [ -n "${EXPECT_MIN:-}" ]; then
    if [ "${MIN:-0}" = "$EXPECT_MIN" ]; then ok "min-instances $MIN"; else bad "min-instances is ${MIN:-0}, the ruling is $EXPECT_MIN"; fi
  else
    ok "min-instances ${MIN:-0} (no ruling given; EXPECT_MIN=<n> to check one)"
  fi
fi

# The image the serving revision resolved to — the read that was missing on 21 Sep.
if [ -n "$REV" ]; then
  DIGEST="$(gcloud run revisions describe "$REV" --project "$PROJECT" --region "$REGION" --format=json 2>/dev/null | jsonq 'b["status"]["imageDigest"]')"
  if [ -n "$DIGEST" ]; then ok "image ${DIGEST##*@}"; else bad "image of $REV cannot be read (artifactregistry.reader?)"; fi
fi

# What a deploy does first: upload the source to the Cloud Build bucket, then submit.
PROBE="$(mktemp)"; printf 'probe\n' > "$PROBE"
if gcloud storage cp "$PROBE" "gs://${PROJECT}_cloudbuild/source/ops-probe-$$.txt" >/dev/null 2>&1; then
  gcloud storage rm "gs://${PROJECT}_cloudbuild/source/ops-probe-$$.txt" >/dev/null 2>&1
  ok "build bucket gs://${PROJECT}_cloudbuild writable"
else
  bad "build bucket gs://${PROJECT}_cloudbuild refuses a write — a deploy as $WHO would fail at upload"
fi
rm -f "$PROBE"
if gcloud builds list --project "$PROJECT" --limit 1 --format='value(id,status)' >/dev/null 2>&1; then
  ok "builds listable"
else
  bad "builds cannot be listed (cloudbuild.builds.editor?)"
fi

# The ward itself: answering, from the revision that is serving, saying the door the service says,
# on a board younger than ten minutes.
PAGE="$(curl -sS -m 90 "$WARD/api/ward" 2>/dev/null)" || PAGE=""
if [ -z "$PAGE" ]; then
  bad "ward $WARD/api/ward cannot be read"
else
  PAGE_REV="$(printf '%s' "$PAGE" | jsonq 'b.get("revision","")')"
  PAGE_DOOR="$(printf '%s' "$PAGE" | jsonq 'b.get("queue",{}).get("door","")')"
  KEPT_AT="$(printf '%s' "$PAGE" | jsonq 'b.get("board",{}).get("kept_at",0)')"
  AGE=$(( $(date +%s) - ${KEPT_AT:-0} ))
  if [ -n "$REV" ] && [ "$PAGE_REV" != "$REV" ]; then
    bad "ward answered from '${PAGE_REV:-?}', the service is serving $REV"
  else
    ok "ward answers from $PAGE_REV"
  fi
  if [ -n "$DOOR" ] && [ "$PAGE_DOOR" != "$DOOR" ]; then
    bad "door on the page is '${PAGE_DOOR:-?}', the service says '$DOOR'"
  else
    ok "door on the page $PAGE_DOOR"
  fi
  if [ "$AGE" -le 600 ]; then ok "board kept ${AGE}s ago"; else bad "board is ${AGE}s old — nothing has rebuilt it in ten minutes"; fi
fi

# The scheduled tick: production has a Cloud Scheduler job every minute; the proof it works is a
# 200 in the request log within the last three minutes, which needs no Scheduler role to read.
if [ "$TICK_EXPECTED" = 1 ]; then
  TICK="$(gcloud logging read "resource.type=\"cloud_run_revision\" AND resource.labels.service_name=\"$SERVICE\" AND httpRequest.requestUrl:\"/api/ward/tick\"" \
      --project "$PROJECT" --limit 1 --freshness=3m --format='value(timestamp,httpRequest.status)' 2>/dev/null)"
  TICK_STATUS="$(printf '%s' "$TICK" | awk -F'\t' 'NR==1{print $2}')"
  if [ -z "$TICK" ]; then bad "tick — no request to /api/ward/tick in the last three minutes"
  elif [ "$TICK_STATUS" != 200 ]; then bad "tick — last answer was $TICK_STATUS, not 200"
  else ok "tick answered 200 at ${TICK%%	*}"; fi
else
  skip "tick — no scheduled job on $TARGET"
fi

# The factory: loaded under launchd, last exit 0, a tick in the log within twenty minutes.
if [ "$TICK_EXPECTED" = 1 ]; then
  FJ="$(launchctl print "gui/$(id -u)/$FACTORY_LABEL" 2>/dev/null)"
  if [ -z "$FJ" ]; then
    bad "factory $FACTORY_LABEL is not loaded"
  else
    FEXIT="$(printf '%s' "$FJ" | awk -F'= ' '/last exit code/ {print $2}' | tr -d ' ')"
    LAST_TICK="$(grep -oE '^\[[0-9]+\] tick done' "$FACTORY_LOG" 2>/dev/null | tail -n 1 | grep -oE '[0-9]+')"
    FAGE=$(( $(date +%s) - ${LAST_TICK:-0} ))
    if [ "$FEXIT" != 0 ] && [ "$FEXIT" != "(never" ]; then bad "factory last exit code $FEXIT"
    elif [ "$FAGE" -gt 1200 ]; then bad "factory last tick ${FAGE}s ago — the job runs every 600 s"
    else ok "factory loaded · last exit ${FEXIT:-?} · last tick ${FAGE}s ago"; fi
  fi
else
  skip "factory — the production job only"
fi

# The chain the ward reads and writes.
HEALTH="$(curl -sS -m 20 -X POST -H 'content-type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' "${RPC:-https://api.devnet.solana.com}" 2>/dev/null | jsonq 'b.get("result","")')"
if [ "$HEALTH" = ok ]; then ok "rpc ${RPC:-?} healthy"; else bad "rpc ${RPC:-?} answered '${HEALTH:-nothing}', not ok"; fi

echo
if [ "$FAILED" -eq 0 ]; then
  echo "every check passed on $TARGET as $WHO."
else
  echo "$FAILED check(s) failed on $TARGET — fix them before the day, not on it."
  exit 1
fi
