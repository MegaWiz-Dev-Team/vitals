#!/usr/bin/env bash
# Put case packs through the ward's door — staging or production — as the ops service account,
# after asking the two questions the door itself does not ask yet.
#
#   scripts/push-case.sh <staging|production> <pack.json> [<pack.json> ...]
#
# Why. On 23 Sep 2026 a recompiled ddx-pneumonia-1-en went through production's door over a copy
# the chain already carried a shift against: a ward closure, anchored the day before, whose leaf
# is hash(case) + tape + receipt. The push changed the case, so the leaf that shift re-derives to
# no longer matched the one on chain — one anchored shift made unverifiable by a content push
# that the door accepted with a 200. The check that was run beforehand asked "is a human shift
# anchored against this case?", and the leaf does not care who signed. So this script asks the
# board the right question for every pack, and refuses before anything is sent:
#
#   - any patient on that case with a shift on chain (a `closed_slot`, or shifts counted) means
#     the case is not replaceable — provisional or reviewed, same version or bumped;
#   - a pack whose version equals the catalogue's for that case is a silent edit, and refused: a
#     replace bumps meta.version, so the catalogue shows that something changed.
#
# It runs as the ops service account by construction (configuration pinned per process, never
# `gcloud config set`), reads the door token into the shell and nowhere else, and prints the
# door's own answer for each pack. Exit 0 only when every pack was stored.
set -uo pipefail

TARGET="${1:-}"; shift || true
case "$TARGET" in
  staging)
    PROJECT=vitals-academy-dev; CONFIG=vitals-ops-dev
    WARD="${VITALS_WARD_URL:-https://vitals-world-367117259093.asia-southeast1.run.app}" ;;
  production)
    PROJECT=vitals-academy; CONFIG=vitals-ops
    WARD="${VITALS_WARD_URL:-https://world.vitals.academy}" ;;
  *)
    echo "refusing: the target is 'staging' or 'production', not '${TARGET:-nothing}'." >&2
    echo "    scripts/push-case.sh <staging|production> <pack.json> [<pack.json> ...]" >&2
    exit 1 ;;
esac
if [ "$#" -eq 0 ]; then
  echo "refusing: no pack named." >&2
  echo "    scripts/push-case.sh <staging|production> <pack.json> [<pack.json> ...]" >&2
  exit 1
fi
EXPECT_ACCOUNT="vitals-ops@${PROJECT}.iam.gserviceaccount.com"

export CLOUDSDK_ACTIVE_CONFIG_NAME="$CONFIG"
WHO="$(gcloud config get-value account 2>/dev/null)"
echo "── as        ${WHO:-(nobody)}  [configuration $CONFIG]"
if [ "$WHO" != "$EXPECT_ACCOUNT" ]; then
  echo "refusing: '${WHO:-nobody}' is a person, or the wrong account — this runs as $EXPECT_ACCOUNT." >&2
  exit 1
fi
if ! gcloud auth print-access-token >/dev/null 2>&1; then
  echo "refusing: no usable credential for $WHO — the key is missing or revoked. Nothing was sent." >&2
  exit 1
fi

# The board and the catalogue, once, before any token is read.
BOARD="$(curl -sS -m 90 "$WARD/api/ward" 2>/dev/null)" || BOARD=""
CATALOGUE="$(curl -sS -m 90 "$WARD/api/ward/cases" 2>/dev/null)" || CATALOGUE=""
if [ -z "$BOARD" ] || [ -z "$CATALOGUE" ]; then
  echo "refusing: $WARD did not answer for the board or the catalogue — the question cannot be asked, so nothing is sent." >&2
  exit 1
fi
echo "── ward      $WARD"

# One read of each pack: its id and version; then the board's answer for that id — every patient
# on the case with a shift on chain — and the catalogue's version for it.
ask() {
  python3 - "$1" "$BOARD" "$CATALOGUE" <<'PY'
import json, sys
pack = json.load(open(sys.argv[1]))
board = json.loads(sys.argv[2]); cat = json.loads(sys.argv[3])
cid = pack.get("case_id", ""); ver = str(pack.get("version", ""))
held = []
for p in board.get("patients", []):
    case = p.get("case")
    case = case if isinstance(case, str) else (case or {}).get("case_id")
    if case != cid:
        continue
    shifts = p.get("shifts")
    n = len(shifts) if isinstance(shifts, list) else int(shifts or 0)
    if p.get("closed_slot") or n > 0:
        held.append(f"{p.get('patient_id')} {p.get('name', '')}".strip())
have = next((str(c.get("version", "")) for c in cat.get("cases", []) if c.get("case_id") == cid), "")
print(cid); print(ver); print(have); print("|".join(held))
PY
}

SENT=0; REFUSED=0; TOKEN=""
for PACK in "$@"; do
  if [ ! -f "$PACK" ]; then
    echo "refusing: $PACK is not a file." >&2; REFUSED=$((REFUSED + 1)); continue
  fi
  { read -r CASE_ID; read -r VERSION; read -r HAVE; read -r HELD; } < <(ask "$PACK")
  if [ -z "$CASE_ID" ]; then
    echo "refusing: $PACK has no case_id." >&2; REFUSED=$((REFUSED + 1)); continue
  fi
  printf '── %-44s %s' "$CASE_ID" "v$VERSION"
  [ -n "$HAVE" ] && printf ' (the door holds v%s)' "$HAVE"
  echo
  if [ -n "$HELD" ]; then
    N="$(printf '%s\n' "$HELD" | tr '|' '\n' | grep -c .)"
    echo "   refusing: the chain carries $N shift(s) against $CASE_ID ($(printf '%s' "$HELD" | tr '|' ';')) — replacing the case would change the leaf those shifts re-derive to. Not sent." >&2
    REFUSED=$((REFUSED + 1)); continue
  fi
  if [ -n "$HAVE" ] && [ "$HAVE" = "$VERSION" ]; then
    echo "   refusing: the door already holds $CASE_ID at v$VERSION — a replace bumps meta.version, so the catalogue shows something changed. Not sent." >&2
    REFUSED=$((REFUSED + 1)); continue
  fi
  # The token, read once, on the first pack that may go — into this shell and nowhere else.
  if [ -z "$TOKEN" ]; then
    TOKEN="$(gcloud secrets versions access latest --secret vitals-door-token --project "$PROJECT" 2>/dev/null)"
    if [ -z "$TOKEN" ]; then
      echo "refusing: the door token could not be read as $WHO. Nothing was sent." >&2
      exit 1
    fi
  fi
  ANSWER="$(curl -sS -m 120 -X POST -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
      --data-binary @"$PACK" -w '\n%{http_code}' "$WARD/api/ward/case" 2>/dev/null)"
  CODE="${ANSWER##*$'\n'}"; BODY="${ANSWER%$'\n'*}"
  if [ "$CODE" = 200 ]; then
    STORED="$(printf '%s' "$BODY" | python3 -c 'import json,sys; b=json.load(sys.stdin); print(b.get("stored","?"), "v"+str(b.get("version","?")), "provisional" if b.get("provisional") else "reviewed")' 2>/dev/null)"
    echo "   stored: ${STORED:-$BODY}"
    SENT=$((SENT + 1))
  else
    echo "   the door answered $CODE: ${BODY:-(nothing)}" >&2
    REFUSED=$((REFUSED + 1))
  fi
done
unset TOKEN

echo "── $SENT sent, $REFUSED refused, on $TARGET as $WHO"
[ "$REFUSED" -eq 0 ]
