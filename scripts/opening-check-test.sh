#!/usr/bin/env bash
# Prove that scripts/opening-check.sh notices each thing an opening day needs and does not have.
#
# On 21 Sep 2026 the service account could read the secrets and describe the service, and the
# opening still slipped two and a half hours, because the one call nobody had tried as that
# account was the one the evening needed. This harness makes each call fail in turn and asserts
# the check names it — so "every call passed" means every call was made.
#
# gcloud, curl and launchctl are stubs; nothing here touches a network, a project or a ward.
#
#   scripts/opening-check-test.sh
set -uo pipefail
cd "$(dirname "$0")/.."

TARGET="$PWD/scripts/opening-check.sh"
# Under the repo rather than the system temp dir: a sandboxed shell on the mini hung every stub
# named launchctl or curl that lived under /var/folders, and the same files ran fine from here.
mkdir -p target
WORK="$(mktemp -d "$PWD/target/opening-check.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bin"

cat > "$WORK/bin/gcloud" <<'STUB'
#!/usr/bin/env bash
set -u
printf '%s\n' "$*" >> "${STUB_CALLS:-/dev/null}"
case "${CLOUDSDK_ACTIVE_CONFIG_NAME:-}" in
  vitals-ops)     ACCT="${STUB_ACCOUNT_PROD-vitals-ops@vitals-academy.iam.gserviceaccount.com}" ;;
  vitals-ops-dev) ACCT="${STUB_ACCOUNT_DEV-vitals-ops@vitals-academy-dev.iam.gserviceaccount.com}" ;;
  *)              ACCT="${STUB_ACCOUNT-someone@example.com}" ;;
esac
case "$1 $2" in
  "config get-value") printf '%s\n' "$ACCT"; exit 0 ;;
  "auth print-access-token")
    [ "${STUB_AUTH_FAIL:-0}" = 1 ] && { echo "ERROR: Reauthentication failed." >&2; exit 1; }
    echo ya29.stub; exit 0 ;;
  "secrets versions")
    [ "${STUB_SECRET_FAIL:-0}" = 1 ] && { echo "ERROR: PERMISSION_DENIED" >&2; exit 1; }
    printf 'stub-secret-value\n'; exit 0 ;;
  "run services")
    printf '{"status":{"latestReadyRevisionName":"%s"},"spec":{"template":{"metadata":{"annotations":{"autoscaling.knative.dev/minScale":"%s"}},"spec":{"containers":[{"env":[{"name":"VITALS_WARD_DOOR","value":"%s"},{"name":"VITALS_RPC","value":"https://rpc.example"}]}]}}}}\n' \
      "${STUB_REV-vitals-world-00026-xzm}" "${STUB_MIN-0}" "${STUB_DOOR-open}"; exit 0 ;;
  "run revisions")
    [ "${STUB_REVISION_FAIL:-0}" = 1 ] && { echo "ERROR: PERMISSION_DENIED" >&2; exit 1; }
    printf '{"status":{"imageDigest":"gcr.io/p/vitals-world@sha256:aaaa"}}\n'; exit 0 ;;
  "storage cp")
    [ "${STUB_BUCKET_FAIL:-0}" = 1 ] && { echo "ERROR: forbidden from accessing the bucket" >&2; exit 1; }
    exit 0 ;;
  "storage rm") exit 0 ;;
  "builds list")
    [ "${STUB_BUILDS_FAIL:-0}" = 1 ] && { echo "ERROR: PERMISSION_DENIED" >&2; exit 1; }
    printf 'stub-build-id\tSUCCESS\n'; exit 0 ;;
  "logging read")
    [ "${STUB_TICK_ABSENT:-0}" = 1 ] && exit 0
    printf '2026-09-21T20:33:00Z\t%s\t0.98s\n' "${STUB_TICK_STATUS-200}"; exit 0 ;;
esac
echo "stub gcloud: unhandled [$*]" >&2
exit 64
STUB

cat > "$WORK/bin/curl" <<'STUB'
#!/usr/bin/env bash
url="${@: -1}"
case "$url" in
  */api/ward)
    [ "${STUB_WARD_DOWN:-0}" = 1 ] && exit 7
    printf '{"revision":"%s","queue":{"door":"%s"},"board":{"from":"chain","kept_at":%s}}\n' \
      "${STUB_PAGE_REV-vitals-world-00026-xzm}" "${STUB_PAGE_DOOR-open}" "${STUB_KEPT_AT-$(date +%s)}"; exit 0 ;;
  *)
    [ "${STUB_RPC_DOWN:-0}" = 1 ] && exit 7
    printf '{"jsonrpc":"2.0","result":"%s","id":1}\n' "${STUB_RPC_RESULT-ok}"; exit 0 ;;
esac
STUB

cat > "$WORK/bin/launchctl" <<'STUB'
#!/usr/bin/env bash
[ "${STUB_FACTORY_MISSING:-0}" = 1 ] && { echo "Could not find service" >&2; exit 113; }
printf '\tstate = not running\n\truns = 9\n\tlast exit code = %s\n' "${STUB_FACTORY_EXIT-0}"
STUB
chmod +x "$WORK/bin/gcloud" "$WORK/bin/curl" "$WORK/bin/launchctl"

PASS=0; FAIL=0; CASE_N=0

# run <name> <expectation> <needle> <target> -- <env assignments...>
run() {
  local name="$1" expect="$2" needle="$3" target="$4"; shift 4
  [ "${1-}" = "--" ] && shift
  CASE_N=$((CASE_N + 1))
  local sandbox="$WORK/case$CASE_N"; mkdir -p "$sandbox/.vitals/world-prod"
  # A factory log whose last tick is fresh unless the case says otherwise (STUB_TICK_AGE=<s>).
  local age=60 a
  for a in "$@"; do case "$a" in STUB_TICK_AGE=*) age="${a#*=}" ;; esac; done
  printf '[%s] tick done: 0 queued\n' "$(( $(date +%s) - age ))" > "$sandbox/.vitals/world-prod/factory.log"
  local out rc
  out="$(env -i PATH="$WORK/bin:/usr/bin:/bin" HOME="$sandbox" TMPDIR="$sandbox" STUB_CALLS="$sandbox/calls" \
      "$@" bash "$TARGET" "$target" 2>&1 </dev/null)"
  rc=$?
  local verdict=ok
  case "$expect" in
    rejects) [ "$rc" -ne 0 ] || verdict="exited 0 when it should have failed" ;;
    accepts) [ "$rc" -eq 0 ] || verdict="exited $rc when it should have passed" ;;
  esac
  if [ "$verdict" = ok ] && ! printf '%s' "$out" | grep -qF -- "$needle"; then
    verdict="never said \"$needle\""
  fi
  if [ "$verdict" = ok ]; then
    printf '  \033[32mpass\033[0m  %s \033[2m(exit %d)\033[0m\n' "$name" "$rc"; PASS=$((PASS + 1))
  else
    printf '  \033[31mFAIL\033[0m  %s — %s\n' "$name" "$verdict"
    printf '%s\n' "$out" | sed 's/^/        | /'; FAIL=$((FAIL + 1))
  fi
}

echo "── opening-check fails when ──"
run "the target is not staging or production" rejects "refusing" demo
run "a person is the active account" rejects "a person" production -- STUB_ACCOUNT_PROD=someone@example.com
run "the credential is expired or missing" rejects "credential" production -- STUB_AUTH_FAIL=1
run "a secret cannot be read" rejects "FAIL  secret vitals-door-token" production -- STUB_SECRET_FAIL=1
run "the revision's image cannot be read" rejects "FAIL  image" production -- STUB_REVISION_FAIL=1
run "the Cloud Build bucket refuses a write" rejects "FAIL  build bucket" production -- STUB_BUCKET_FAIL=1
run "builds cannot be listed" rejects "FAIL  builds" production -- STUB_BUILDS_FAIL=1
run "the ward cannot be read" rejects "FAIL  ward" production -- STUB_WARD_DOWN=1
run "the ward answers from a revision that is not the serving one" rejects "FAIL  ward" production -- STUB_PAGE_REV=vitals-world-00019-old
run "the page's door is not the service's door" rejects "FAIL  door" production -- STUB_PAGE_DOOR=preview
run "the board is older than ten minutes" rejects "FAIL  board" production -- STUB_KEPT_AT=1700000000
run "no tick answered in the last three minutes" rejects "FAIL  tick" production -- STUB_TICK_ABSENT=1
run "the last tick did not answer 200" rejects "FAIL  tick" production -- STUB_TICK_STATUS=500
run "the factory job is not loaded" rejects "FAIL  factory" production -- STUB_FACTORY_MISSING=1
run "the factory's last exit was not 0" rejects "FAIL  factory" production -- STUB_FACTORY_EXIT=1
run "the factory has not ticked for 25 minutes" rejects "FAIL  factory" production -- STUB_TICK_AGE=1500
run "the RPC does not answer ok" rejects "FAIL  rpc" production -- STUB_RPC_RESULT=behind
run "min-instances is not what the founder ruled" rejects "FAIL  min-instances" production -- STUB_MIN=1 EXPECT_MIN=0

echo "── opening-check passes when ──"
run "every call passes on production" accepts "every check passed" production
run "staging: the tick and the factory are not expected, and it says so" accepts "skip  tick" staging
run "the door is preview and the page agrees" accepts "every check passed" staging -- STUB_DOOR=preview STUB_PAGE_DOOR=preview
run "min-instances matches the ruling given" accepts "ok    min-instances 1" production -- STUB_MIN=1 EXPECT_MIN=1

echo
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
