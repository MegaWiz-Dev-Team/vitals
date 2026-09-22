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
# The script refuses SERVICE=vitals from a cwf/* branch and SERVICE=vitals-world from anywhere
# else (its branch guard), and it refuses before it reaches anything these cases are about. So the
# harness names the service the branch is allowed to deploy — otherwise every case here dies at
# that guard and reports "never said …", which is what happened the first time it was run from
# cwf/ward. A harness nobody runs is a harness that rots; this one is now in gates.
BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo main)"
case "$BRANCH" in cwf/*) SERVICE=vitals-world ;; *) SERVICE=vitals ;; esac
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
    # The submit only names the build; what happened to it is a fact of the build, read back
    # by id. Faithful to the two ways the real thing has lied: the log stream can fail for a
    # caller who is not a project Viewer (exit 1, build green), and the last line of stdout is
    # not always the value asked for.
    echo "Created [https://cloudbuild.googleapis.com/v1/projects/p/locations/global/builds/${STUB_BUILD_ID-b1d0}]." >&2
    echo "Logs are available at [ https://console.cloud.google.com/stub ]." >&2
    [ "${STUB_SUBMIT_FAIL:-0}" = 1 ] && { echo "ERROR: (gcloud.builds.submit) stub submit failure" >&2; exit 1; }
    case " $* " in
      *" --async "*) printf '%s\n' "${STUB_BUILD_ID-b1d0}"; exit 0 ;;
    esac
    echo "stub gcloud: builds submit without --async is the old shape" >&2; exit 64 ;;

  "builds log")
    if [ "${STUB_CANNOT_STREAM:-0}" = 1 ]; then
      echo "ERROR: (gcloud.builds.log) This tool can only stream logs if you are Viewer/Owner of the project" >&2
      exit 1
    fi
    if [ "${STUB_BUILD_LOG:-1}" = 1 ]; then
      echo "Step #0: Pulling image gcr.io/cloud-builders/docker"
      echo "Step #0: DONE"
    fi
    exit 0 ;;

  "builds describe")
    [ "${STUB_DESCRIBE_FAIL:-0}" = 1 ] && { echo "ERROR: (gcloud.builds.describe) stub: cannot reach the build" >&2; exit 1; }
    # STATUS<TAB>DIGEST, the two fields the script asks for; a build that failed names no image.
    if [ "${STUB_BUILD_FAIL:-0}" = 1 ]; then printf 'FAILURE\t\n'; exit 0; fi
    # STUB_BUILD_NO_VALUE: the value line is missing altogether, as a format drift would look.
    [ "${STUB_BUILD_NO_VALUE:-0}" = 1 ] && exit 0
    printf 'SUCCESS\t%s\n' "${STUB_BUILD_DIGEST-sha256:aaaa}"; exit 0 ;;

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
      SERVICE="$SERVICE" \
      VITALS_NO_VOICE="${VITALS_NO_VOICE-1}" \
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

# The voice guard (a mute deploy on 30 Aug–4 Sep — patients who could not speak for five days)
# refuses a deploy with neither VITALS_VERTEX_URL nor HEIMDALL_API_URL. Stubbed runs say
# VITALS_NO_VOICE=1 on purpose, so this one case is where the guard is seen to fire.
run "a bay whose patients cannot speak is refused" rejects "cannot speak" \
  -- VITALS_NO_VOICE=

run "no account at all" rejects "no active account" \
  -- STUB_ACCOUNT=
run "a service account is still not you" rejects "not you" \
  -- STUB_ACCOUNT=deployer@x.iam.gserviceaccount.com
# The founder approved one deploy/factory identity on 20 ก.ย. — `vitals-ops` in each project — so
# that a 03:00 deploy does not need his login, which expires in about twelve hours. Exactly those
# two names pass the guard. Every other service account is refused as before: an allowlist by
# name, never by suffix, or the next stray credential deploys production because it ends in the
# right domain.
run "the ops service account may deploy (prod project)" accepts "vitals-ops@vitals-academy.iam.gserviceaccount.com" \
  -- STUB_ACCOUNT=vitals-ops@vitals-academy.iam.gserviceaccount.com
run "the ops service account may deploy (dev project)" accepts "vitals-ops@vitals-academy-dev.iam.gserviceaccount.com" \
  -- STUB_ACCOUNT=vitals-ops@vitals-academy-dev.iam.gserviceaccount.com VITALS_GCP_PROJECT=vitals-academy-dev
# gcloud's exit code and the build's status are different questions. The service account is not
# a project Viewer, so gcloud cannot stream the log to it and exits 1 with the build green — which
# is how the first two deploys as the account died on 22 Sep with a finished image nobody deployed
# (and --suppress-logs did not move it). The script asks the build what happened, by id, and
# says what it concluded and from what.
run "the log stream failing is not the build failing" accepts "build b1d0: SUCCESS" \
  -- STUB_ACCOUNT=vitals-ops@vitals-academy-dev.iam.gserviceaccount.com VITALS_GCP_PROJECT=vitals-academy-dev STUB_CANNOT_STREAM=1
run "as a person, the build log is still streamed" accepts "Step #0: DONE" \
  -- STUB_ACCOUNT=someone@example.com STUB_CANNOT_STREAM=0
run "a build that failed is refused by its status, not by an exit code" rejects "build b1d0: FAILURE" \
  -- STUB_BUILD_FAIL=1
run "a submit that names no build stops before anything is built" rejects "named no build" \
  -- STUB_SUBMIT_FAIL=1
# A status that cannot be read is not a failed build and not a successful one; it is unknown,
# and the safe answer is to stop and say so rather than assume either way.
run "a build whose status cannot be read is unknown, not failed" rejects "unknown" \
  -- STUB_DESCRIBE_FAIL=1
# Two settings a deploy speaks for whether or not anybody named them. `--set-env-vars` replaces
# the whole environment, so an arrival interval set by hand was silently dropped by the next
# deploy (22 Sep, four minutes after the founder ruled 60); and `--concurrency` hard-coded at 8
# put the platform's belief about the container on no screen at all, while every open tab held a
# 301 s stream against it and strangers were refused. Both are named on every deploy and printed.
run "the arrival interval rides on every deploy and is printed" accepts "── arrivals  every 60 min" \
  -- VITALS_WARD_ARRIVAL_MINUTES=60
run "the arrival interval defaults to the binary's own 30 and says so" accepts "── arrivals  every 30 min" \
  --
run "concurrency is printed next to the service" accepts "── concurrency 80 requests in flight per instance" \
  --
run "concurrency can be set, and the deploy says what it set" accepts "── concurrency 40 requests in flight per instance" \
  -- CONCURRENCY=40
run "the same name in another project is still not you" rejects "not you" \
  -- STUB_ACCOUNT=vitals-ops@some-other-project.iam.gserviceaccount.com
run "a look-alike is still not you" rejects "not you" \
  -- STUB_ACCOUNT=vitals-ops@vitals-academy.iam.gserviceaccount.com.evil.example

run "a build that names no image" rejects "no image digest" \
  -- STUB_BUILD_DIGEST= STUB_BUILD_LOG=0
# The build log shares stdout with the --format value, so a capture that drifted onto a log
# line has to say so rather than fail later as a digest mismatch and send someone hunting a
# stale image that does not exist.
run "the build's answer is not a digest" rejects "was not a digest" \
  -- STUB_BUILD_DIGEST=gcr.io/p/not-a-digest
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
