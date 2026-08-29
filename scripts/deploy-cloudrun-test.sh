#!/usr/bin/env bash
# Prove that scripts/deploy-cloudrun.sh fails when it should.
#
# This exists because that script once exited 0 having built nothing and deployed nothing: gcloud
# asked to re-authenticate in a shell with nobody to answer, and the run was reported as a
# success. Everything downstream of a deploy script believes its exit code, and downstream here
# ends at a clinician clicking a link. So the checks that catch that are not allowed to be
# checks nobody has ever seen fire.
#
# gcloud is replaced by a stub, so nothing here touches a network, a project, or a bill. Each
# case configures the stub to misbehave in one specific way and asserts the script notices.
#
#   scripts/deploy-cloudrun-test.sh
set -uo pipefail
cd "$(dirname "$0")/.."

TARGET="$PWD/scripts/deploy-cloudrun.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# The stub reads its instructions from the environment, so a case is a set of env vars. Anything
# not named behaves like a healthy deploy — a case should differ from the happy path in exactly
# the one way it is about.
mkdir -p "$WORK/bin"
cat > "$WORK/bin/gcloud" <<'STUB'
#!/usr/bin/env bash
set -u
say_json() { printf '%s\n' "$1"; }

case "$1 $2" in
  "config get-value")
    printf '%s\n' "${STUB_ACCOUNT-someone@example.com}"; exit 0 ;;

  "auth print-access-token")
    if [ "${STUB_AUTH_FAIL:-0}" = 1 ]; then
      cat >&2 <<'MSG'
ERROR: (gcloud.auth.print-access-token) There was a problem refreshing your current auth tokens: Reauthentication failed. cannot prompt during non-interactive execution.
Please run:

  $ gcloud auth login

to obtain new credentials.
MSG
      exit 1
    fi
    echo "ya29.stub-access-token"; exit 0 ;;

  "builds submit")
    [ "${STUB_BUILD_FAIL:-0}" = 1 ] && { echo "ERROR: (gcloud.builds.submit) stub build failure" >&2; exit 1; }
    echo "Logs are available at [ https://console.cloud.google.com/stub ]." >&2
    # Faithful to the real thing in the way that matters here: gcloud streams the build log on
    # STDOUT (submit_util.py: out = log.out), the same stream the --format value lands on.
    if [ "${STUB_BUILD_LOG:-1}" = 1 ]; then
      echo "Step #0: Pulling image gcr.io/cloud-builders/docker"
      echo "Step #0: DONE"
    fi
    # STUB_BUILD_NO_VALUE: gcloud prints no --format value at all, so the last line of stdout
    # is a build log line — what a capture that has drifted onto the wrong stream would see.
    [ "${STUB_BUILD_NO_VALUE:-0}" = 1 ] || printf '%s\n' "${STUB_BUILD_DIGEST-sha256:aaaa}"
    exit 0 ;;

  "run deploy")
    [ "${STUB_DEPLOY_FAIL:-0}" = 1 ] && { echo "ERROR: (gcloud.run.deploy) stub deploy failure" >&2; exit 1; }
    echo "stub: deploying..." >&2
    printf '%s\n' "${STUB_REVISION-vitals-00007-abc}"
    exit 0 ;;

  "run services")
    say_json "{
      \"status\": {
        \"url\": \"${STUB_URL-https://vitals.example.run.app}\",
        \"traffic\": [ { \"percent\": 100, \"revisionName\": \"${STUB_TRAFFIC_REV-vitals-00007-abc}\" } ]
      }
    }"
    exit 0 ;;

  "run revisions")
    say_json "{ \"status\": { \"imageDigest\": \"gcr.io/p/vitals@${STUB_REVISION_DIGEST-sha256:aaaa}\" } }"
    exit 0 ;;

  "container images")
    [ -z "${STUB_TAG_DIGEST-sha256:aaaa}" ] && exit 1
    printf '%s\n' "${STUB_TAG_DIGEST-sha256:aaaa}"; exit 0 ;;
esac
echo "stub gcloud: unhandled [$*]" >&2
exit 64
STUB
chmod +x "$WORK/bin/gcloud"

PASS=0; FAIL=0
CASE_N=0

# run <name> <expectation> <needle> -- <env assignments...>
#   expectation is `rejects` or `accepts`; needle must appear in the output either way.
run() {
  local name="$1" expect="$2" needle="$3"; shift 3
  [ "$1" = "--" ] && shift
  CASE_N=$((CASE_N + 1))
  local sandbox="$WORK/case$CASE_N"; mkdir -p "$sandbox"

  local out rc
  out="$(env -i \
      PATH="$WORK/bin:/usr/bin:/bin" \
      HOME="$sandbox" TMPDIR="$sandbox" \
      VITALS_GCP_PROJECT=vitals-academy \
      VITALS_PROGRAM_ID=535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG \
      "$@" \
      bash "$TARGET" 2>&1 </dev/null)"
  rc=$?

  local verdict=ok
  case "$expect" in
    rejects) [ "$rc" -ne 0 ] || verdict="exited 0 when it should have refused" ;;
    accepts) [ "$rc" -eq 0 ] || verdict="exited $rc when it should have passed" ;;
  esac
  if [ "$verdict" = ok ] && ! printf '%s' "$out" | grep -qF "$needle"; then
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

echo "── deploy-cloudrun refuses ──"

# The original bug, exactly: credentials on disk, unrefreshable, non-interactive shell.
run "expired credentials, before the build" rejects "gcloud auth login" \
  -- STUB_AUTH_FAIL=1
# ...and it must not have reached the build. A stub build that would have exploded proves it.
run "expired credentials cost no build" rejects "will not refresh" \
  -- STUB_AUTH_FAIL=1 STUB_BUILD_FAIL=1

run "no account at all" rejects "no active account" \
  -- STUB_ACCOUNT=
run "a service account is still not you" rejects "not you" \
  -- STUB_ACCOUNT=deployer@x.iam.gserviceaccount.com

run "a build that names no image" rejects "no image digest" \
  -- STUB_BUILD_DIGEST= STUB_BUILD_LOG=0
# The build log shares stdout with the --format value, so a capture that drifted onto a log
# line has to say so rather than fail later as a digest mismatch and send someone hunting a
# stale image that does not exist.
run "the build's last word is not a digest" rejects "was not a digest" \
  -- STUB_BUILD_NO_VALUE=1
run "a deploy that names no revision" rejects "nothing to verify" \
  -- STUB_REVISION=

run "an older revision keeps the traffic" rejects "NOT SERVING" \
  -- STUB_REVISION=vitals-00008-new STUB_TRAFFIC_REV=vitals-00007-old \
     STUB_REVISION_DIGEST=sha256:aaaa
run "the revision runs a stale image" rejects "STALE IMAGE" \
  -- STUB_BUILD_DIGEST=sha256:fresh STUB_REVISION_DIGEST=sha256:yesterday
run "Cloud Run records no digest" rejects "NO DIGEST" \
  -- STUB_REVISION_DIGEST=
run "no anchor to compare the image against" rejects "UNVERIFIED" \
  -- PHASE=deploy STUB_TAG_DIGEST=
run "a service with no URL" rejects "NO URL" \
  -- STUB_URL=

echo "── and passes work that was actually done ──"

# STUB_DEPLOY_FAIL is the proof that PHASE=build never reaches the deploy: if it did, the run
# would not be green.
run "PHASE=build stops after the build" accepts "── built     sha256:fresh" \
  -- PHASE=build STUB_BUILD_DIGEST=sha256:fresh STUB_DEPLOY_FAIL=1
# Capturing the digest must not cost the build log; it is the only thing to watch for twenty
# minutes, and a check bought with someone's visibility is the trade this file exists to refuse.
run "the build log still reaches the terminal" accepts "Step #0: DONE" \
  -- PHASE=build STUB_BUILD_DIGEST=sha256:fresh STUB_DEPLOY_FAIL=1
run "PHASE=all, fresh image, serving" accepts "https://vitals.example.run.app" \
  -- STUB_BUILD_DIGEST=sha256:fresh STUB_REVISION_DIGEST=sha256:fresh
run "PHASE=deploy checks against the tag" accepts "points at right now" \
  -- PHASE=deploy STUB_TAG_DIGEST=sha256:tagged STUB_REVISION_DIGEST=sha256:tagged

echo
if [ "$FAIL" -eq 0 ]; then
  echo "   $PASS cases, every check fires on demand"
else
  echo "   $FAIL of $((PASS + FAIL)) cases did not behave as claimed"
fi
exit $([ "$FAIL" -eq 0 ] && echo 0 || echo 1)
