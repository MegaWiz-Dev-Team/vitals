#!/usr/bin/env bash
# Prove that scripts/ward-door.sh refuses what it should and verifies what it does.
#
# This exists because the ward opened two and a half hours late on 21 Sep 2026: the one way to
# flip the door was a person's gcloud login, and at 18:52 that login had expired again. The door
# script runs as the ops service account by construction, changes nothing but the one variable,
# and reads the door back off the running ward before it says "done" — and each of those is a
# case here, because a rule that is not a test has a shelf life.
#
# gcloud and curl are replaced by stubs, so nothing here touches a network, a project, or a
# public ward. Each case configures a stub to misbehave in one way and asserts the script notices.
#
#   scripts/ward-door-test.sh
set -uo pipefail
cd "$(dirname "$0")/.."

TARGET="$PWD/scripts/ward-door.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/bin"
# The stub reads its instructions from the environment and records every gcloud call in
# STUB_CALLS, so a case can assert not only what was said but what was and was not done.
cat > "$WORK/bin/gcloud" <<'STUB'
#!/usr/bin/env bash
set -u
printf '%s\n' "$*" >> "${STUB_CALLS:-/dev/null}"
say_json() { printf '%s\n' "$1"; }
account() {
  # The account is whatever the active configuration says, which is how the real thing works;
  # the script is expected to pin the configuration, and the stub answers per configuration.
  case "${CLOUDSDK_ACTIVE_CONFIG_NAME:-}" in
    vitals-ops)     printf '%s\n' "${STUB_ACCOUNT_PROD-vitals-ops@vitals-academy.iam.gserviceaccount.com}" ;;
    vitals-ops-dev) printf '%s\n' "${STUB_ACCOUNT_DEV-vitals-ops@vitals-academy-dev.iam.gserviceaccount.com}" ;;
    *)              printf '%s\n' "${STUB_ACCOUNT-someone@example.com}" ;;
  esac
}
service_json() {
  say_json "{
    \"status\": { \"latestReadyRevisionName\": \"${STUB_CURRENT_REV-vitals-world-00025-pdl}\" },
    \"spec\": { \"template\": { \"spec\": { \"containers\": [ {
      \"image\": \"${STUB_IMAGE-gcr.io/p/vitals-world@sha256:aaaa}\",
      \"env\": [ { \"name\": \"VITALS_WORLD\", \"value\": \"1\" },
               { \"name\": \"VITALS_WARD_DOOR\", \"value\": \"${STUB_CURRENT_DOOR-preview}\" } ]
    } ] } } }
  }"
}
case "$1 $2" in
  "config get-value") account; exit 0 ;;
  "auth print-access-token")
    if [ "${STUB_AUTH_FAIL:-0}" = 1 ]; then
      echo "ERROR: (gcloud.auth.print-access-token) There was a problem refreshing your current auth tokens: Reauthentication failed. cannot prompt during non-interactive execution." >&2
      exit 1
    fi
    echo "ya29.stub"; exit 0 ;;
  "run services")
    case "$3" in
      describe) service_json; exit 0 ;;
      update)
        [ "${STUB_UPDATE_FAIL:-0}" = 1 ] && { echo "ERROR: (gcloud.run.services.update) stub update failure" >&2; exit 1; }
        echo "stub: updating..." >&2
        printf '%s\n' "${STUB_NEW_REV-vitals-world-00026-xzm}"; exit 0 ;;
    esac ;;
  "run revisions")
    say_json "{ \"spec\": { \"containers\": [ { \"image\": \"${STUB_NEW_IMAGE-gcr.io/p/vitals-world@sha256:aaaa}\" } ] } }"
    exit 0 ;;
esac
echo "stub gcloud: unhandled [$*]" >&2
exit 64
STUB
chmod +x "$WORK/bin/gcloud"

# The ward, as the script reads it back after the flip.
cat > "$WORK/bin/curl" <<'STUB'
#!/usr/bin/env bash
[ "${STUB_WARD_DOWN:-0}" = 1 ] && exit 7
printf '{"revision":"%s","queue":{"door":"%s"},"policy":{"catalogue":{"status":"%s"}},"board":{"from":"%s","kept_by":"%s"}}\n' \
  "${STUB_NEW_REV-vitals-world-00026-xzm}" "${STUB_PAGE_DOOR-preview}" "${STUB_PAGE_STATUS-provisional — not yet open for play}" \
  "${STUB_BOARD_FROM-chain}" "${STUB_BOARD_KEPT_BY-}"
STUB
chmod +x "$WORK/bin/curl"

PASS=0; FAIL=0; CASE_N=0

# run <name> <expectation> <needle> <word> <target> -- <env assignments...>
#   expectation is `rejects` or `accepts`; needle must appear in the output either way.
run() {
  local name="$1" expect="$2" needle="$3" word="$4" target="$5"; shift 5
  [ "${1-}" = "--" ] && shift
  CASE_N=$((CASE_N + 1))
  local sandbox="$WORK/case$CASE_N"; mkdir -p "$sandbox"
  : > "$sandbox/calls"

  local out rc
  out="$(env -i \
      PATH="$WORK/bin:/usr/bin:/bin" \
      HOME="$sandbox" TMPDIR="$sandbox" \
      STUB_CALLS="$sandbox/calls" \
      "$@" \
      bash "$TARGET" "$word" "$target" 2>&1 </dev/null)"
  rc=$?
  LAST_CALLS="$sandbox/calls"

  local verdict=ok
  case "$expect" in
    rejects) [ "$rc" -ne 0 ] || verdict="exited 0 when it should have refused" ;;
    accepts) [ "$rc" -eq 0 ] || verdict="exited $rc when it should have passed" ;;
  esac
  if [ "$verdict" = ok ] && ! printf '%s' "$out" | grep -qF -- "$needle"; then
    verdict="never said \"$needle\""
  fi

  if [ "$verdict" = ok ]; then
    printf '  \033[32mpass\033[0m  %s \033[2m(exit %d)\033[0m\n' "$name" "$rc"
    PASS=$((PASS + 1))
  else
    printf '  \033[31mFAIL\033[0m  %s — %s\n' "$name" "$verdict"
    printf '%s\n' "$out" | sed 's/^/        | /'
    FAIL=$((FAIL + 1))
  fi
}

# never_called <gcloud verb words> — the last case must not have made this call.
never_called() {
  if grep -qF -- "$1" "$LAST_CALLS"; then
    printf '  \033[31mFAIL\033[0m  … and it must not have called [%s]\n' "$1"
    FAIL=$((FAIL + 1)); PASS=$((PASS - 1))
  fi
}

echo "── ward-door refuses ──"

run "a word that is not open, preview or closed" rejects "refusing" ajar staging
never_called "run services update"

run "a target that is not staging or production" rejects "refusing" open demo
never_called "run services update"

run "a person as the active account" rejects "a person" preview staging -- \
  STUB_ACCOUNT_DEV=someone@example.com
never_called "run services update"

run "the other project's service account" rejects "not the one for" preview staging -- \
  STUB_ACCOUNT_DEV=vitals-ops@vitals-academy.iam.gserviceaccount.com
never_called "run services update"

run "opening production without the founder's word" rejects "founder" open production
never_called "run services update"

run "an expired or missing credential" rejects "credential" preview staging -- STUB_AUTH_FAIL=1
never_called "run services update"

run "an update that fails" rejects "update" preview staging -- STUB_UPDATE_FAIL=1

run "a revision whose image is not the one that was running" rejects "image" preview staging -- \
  STUB_NEW_IMAGE="gcr.io/p/vitals-world@sha256:bbbb"

run "a ward that cannot be read back" rejects "read back" preview staging -- STUB_WARD_DOWN=1

run "a page that still shows the old door" rejects "kept board" open staging -- \
  STUB_CURRENT_DOOR=preview STUB_PAGE_DOOR=preview STUB_BOARD_FROM=store STUB_BOARD_KEPT_BY=vitals-world-00072-6fc

echo "── ward-door accepts ──"

run "staging to preview, read back as preview" accepts "door on the page: preview" preview staging -- \
  STUB_CURRENT_DOOR=open STUB_PAGE_DOOR=preview

run "staging already at the word: nothing to do" accepts "already" preview staging -- \
  STUB_CURRENT_DOOR=preview STUB_PAGE_DOOR=preview
never_called "run services update"

run "production open on the founder's word, and the word is on the record" accepts "founder's word: เปิดได้เลย" open production -- \
  FOUNDER_WORD="เปิดได้เลย" STUB_CURRENT_DOOR=preview STUB_PAGE_DOOR=open STUB_PAGE_STATUS="provisional — open for play since 21 Sep 2026"

run "production to closed needs no word — shut is the safe direction" accepts "door on the page: closed" closed production -- \
  STUB_CURRENT_DOOR=open STUB_PAGE_DOOR=closed

run "the new revision is named" accepts "vitals-world-00026-xzm" preview staging -- \
  STUB_CURRENT_DOOR=open STUB_PAGE_DOOR=preview

echo
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
