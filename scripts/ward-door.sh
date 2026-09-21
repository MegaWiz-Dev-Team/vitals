#!/usr/bin/env bash
# Flip the ward's door — and nothing else — on staging or production, as the ops service
# account, and read the door back off the running ward before saying so.
#
#   scripts/ward-door.sh <open|preview|closed> <staging|production>
#
# Why this exists. On 21 Sep 2026 the ward opened at 21:30 instead of 19:00 because the one way
# to flip the door was a deploy under a person's gcloud login, and at 18:52 that login had
# expired again. The door is one environment variable on an image that is already built and
# serving; changing it needs no build, no code and no person. So this script:
#
#   - runs as the ops service account by construction (it pins the gcloud configuration itself,
#     `vitals-ops` or `vitals-ops-dev`, and refuses if the account that answers is anybody else);
#   - changes VITALS_WARD_DOOR and nothing else, and refuses if the revision it made runs a
#     different image from the one that was serving — a flip is not a deploy;
#   - opens production only with the founder's word in FOUNDER_WORD, printed to the record;
#     shut and preview are the safe direction and need no word;
#   - reads /api/ward back and fails if the page does not say the door it just set. A board kept
#     from a previous revision used to carry the old door with it (measured on staging 00072/73,
#     22 Sep 2026); the ward stamps the door on the way out now, and this is where that is checked
#     on the real path every time the door moves.
#
# What it is not: a way to change code on production. That is scripts/deploy-cloudrun.sh, from
# a committed revision the producer has read.
set -uo pipefail

WORD="${1:-}"
TARGET="${2:-}"

case "$WORD" in
  open|preview|closed) ;;
  *)
    echo "refusing: the door has three words — open, preview, closed — and '${WORD:-nothing}' is not one of them." >&2
    echo "    scripts/ward-door.sh <open|preview|closed> <staging|production>" >&2
    exit 1 ;;
esac

case "$TARGET" in
  staging)
    PROJECT=vitals-academy-dev
    CONFIG=vitals-ops-dev
    WARD="${VITALS_WARD_URL:-https://vitals-world-367117259093.asia-southeast1.run.app}" ;;
  production)
    PROJECT=vitals-academy
    CONFIG=vitals-ops
    WARD="${VITALS_WARD_URL:-https://world.vitals.academy}" ;;
  *)
    echo "refusing: the target is 'staging' or 'production', not '${TARGET:-nothing}'." >&2
    exit 1 ;;
esac
SERVICE=vitals-world
REGION=asia-southeast1
EXPECT_ACCOUNT="vitals-ops@${PROJECT}.iam.gserviceaccount.com"

# The founder's word, on the record, before anything is touched. Opening a public ward is his
# call and this is where it is quoted; the other two words are the safe direction.
if [ "$TARGET" = production ] && [ "$WORD" = open ]; then
  if [ -z "${FOUNDER_WORD:-}" ]; then
    echo "refusing: production opens only on the founder's word. Put what he said in FOUNDER_WORD:" >&2
    echo "    FOUNDER_WORD='เปิดได้เลย' scripts/ward-door.sh open production" >&2
    exit 1
  fi
  echo "── founder's word: $FOUNDER_WORD"
fi

# Pinned per process, never `gcloud config set`, which is global to every session on the machine.
export CLOUDSDK_ACTIVE_CONFIG_NAME="$CONFIG"
WHO="$(gcloud config get-value account 2>/dev/null)"
echo "── as        ${WHO:-(nobody)}  [configuration $CONFIG]"
case "$WHO" in
  "$EXPECT_ACCOUNT") ;;
  *gserviceaccount.com*)
    echo "refusing: $WHO is a service account, but not the one for $PROJECT ($EXPECT_ACCOUNT)." >&2
    echo "The configuration '$CONFIG' is set up wrong; see the service-account notes." >&2
    exit 1 ;;
  *)
    echo "refusing: '${WHO:-nobody}' is a person, or nobody. This script runs as $EXPECT_ACCOUNT so that" >&2
    echo "an expired login cannot hold the door. Create the configuration once:" >&2
    echo "    gcloud config configurations create $CONFIG && gcloud auth activate-service-account --key-file ~/.vitals/keys/vitals-ops-$PROJECT.json --configuration $CONFIG" >&2
    exit 1 ;;
esac

if ! gcloud auth print-access-token >/dev/null 2>&1; then
  echo "refusing: no usable credential for $WHO — the key is missing or revoked. Nothing was changed." >&2
  exit 1
fi

# What is serving right now: the image, the revision, the door.
SERVICE_JSON="$(gcloud run services describe "$SERVICE" --project "$PROJECT" --region "$REGION" --format=json 2>/dev/null)" || {
  echo "refusing: could not describe $SERVICE in $PROJECT." >&2; exit 1; }
read -r CURRENT_REV CURRENT_IMAGE CURRENT_DOOR < <(printf '%s' "$SERVICE_JSON" | python3 -c '
import json, sys
s = json.load(sys.stdin)
c = s["spec"]["template"]["spec"]["containers"][0]
door = next((e.get("value", "") for e in c.get("env", []) if e.get("name") == "VITALS_WARD_DOOR"), "closed")
print(s["status"].get("latestReadyRevisionName", "?"), c["image"], door or "closed")
')
echo "── serving   $CURRENT_REV"
echo "── image     $CURRENT_IMAGE"
echo "── door      $CURRENT_DOOR"

if [ "$CURRENT_DOOR" = "$WORD" ]; then
  echo "── the door is already $WORD on $CURRENT_REV — nothing to do."
  exit 0
fi

# The one change. --update-env-vars touches this variable alone and keeps the rest of the
# environment and the image as they are; the revision it makes is checked below to be exactly that.
echo "── door      $CURRENT_DOOR → $WORD on $TARGET"
NEW_REV="$(gcloud run services update "$SERVICE" --project "$PROJECT" --region "$REGION" \
    --update-env-vars "VITALS_WARD_DOOR=$WORD" --quiet \
    --format='value(status.latestReadyRevisionName)' 2>/dev/null | tail -n 1)"
if [ -z "$NEW_REV" ]; then
  echo "the update failed or named no revision — the door may or may not have moved; read it before trying again:" >&2
  echo "    gcloud run services describe $SERVICE --project $PROJECT --region $REGION --format='value(spec.template.spec.containers[0].env)'" >&2
  exit 1
fi
echo "── revision  $NEW_REV"

NEW_IMAGE="$(gcloud run revisions describe "$NEW_REV" --project "$PROJECT" --region "$REGION" --format=json 2>/dev/null \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["spec"]["containers"][0]["image"])' 2>/dev/null)"
if [ "$NEW_IMAGE" != "$CURRENT_IMAGE" ]; then
  echo "the new revision runs a different image from the one that was serving:" >&2
  echo "    was  $CURRENT_IMAGE" >&2
  echo "    now  $NEW_IMAGE" >&2
  echo "A flip is not a deploy. Route traffic back to $CURRENT_REV and find out what else changed." >&2
  exit 1
fi

# Read the door back off the ward itself. The first request after a flip lands on a fresh
# instance that loads the board the previous revision kept; the door on that page has to be the
# live one, not the kept one, and this is the check on the real path.
PAGE="$(curl -sS -m 90 "$WARD/api/ward" 2>/dev/null)" || {
  echo "the door moved ($NEW_REV) but the ward could not be read back at $WARD/api/ward — look at it before telling anyone." >&2
  exit 2; }
read -r PAGE_DOOR PAGE_FROM PAGE_KEPT_BY < <(printf '%s' "$PAGE" | python3 -c '
import json, sys
b = json.load(sys.stdin)
print(b.get("queue", {}).get("door", "?"), b.get("board", {}).get("from", "?"), b.get("board", {}).get("kept_by") or "-")
' 2>/dev/null)
PAGE_STATUS="$(printf '%s' "$PAGE" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("policy",{}).get("catalogue",{}).get("status","?"))' 2>/dev/null)"
echo "── door on the page: ${PAGE_DOOR:-?}  (board from ${PAGE_FROM:-?}${PAGE_KEPT_BY:+, kept by $PAGE_KEPT_BY})"
echo "── status: ${PAGE_STATUS:-?}"
if [ "$PAGE_DOOR" != "$WORD" ]; then
  echo "the door is $WORD on $NEW_REV but the page says '$PAGE_DOOR' — a kept board from $PAGE_KEPT_BY is being served with its old door." >&2
  echo "The ward should stamp the live door on the way out; until it does, one pass through POST /api/ward/tick rebuilds the board." >&2
  exit 2
fi
echo "── done: $TARGET door is $WORD on $NEW_REV, and the page says so."
