#!/usr/bin/env bash
# Put the server on Cloud Run, in a project of its own.
#
# The whole product is one Rust binary — page, API and relay together — so there is no separate
# frontend to host. That is a consequence of the relay paying fees: a static host cannot hold a
# signing key, and holding one is what lets a medical student play without ever touching crypto.
#
# A note on how this script reports, because it got that wrong once. `set -e` only governs the
# commands *this* script runs; it says nothing about the pipeline the script itself sits in. Run
# as `deploy-cloudrun.sh 2>&1 | tee log`, the pipeline's status is tee's, and a build that died
# on an expired credential is indistinguishable from a deploy that worked. The same swallow used
# to live inside the script too: `echo "$(gcloud ... describe ...)"` reports echo's status, not
# gcloud's, and it was the last line, so a failed describe made the whole run exit 0. Both are
# gone. Every status below is either checked or assigned to a variable, and the deploy now proves
# what it did rather than assuming it — because the caller of a deploy script believes the exit
# code, and the people downstream of the caller are clinicians clicking a link.
set -euo pipefail
cd "$(dirname "$0")/.."

# Deliberately has no default. embla-cloud's project holds identified users — line_user_id,
# display_name, a profile per person — collected under a stated consent version. A Cloud Run
# service gets its Firestore token from the metadata server, and that token is scoped to the
# project, so a service deployed alongside them can read those documents whether or not it
# ever means to. Those people consented to Embla. Keeping this product in its own project
# makes that separation the default rather than something IAM has to be talked into.
PROJECT="${VITALS_GCP_PROJECT:-}"
REGION="${REGION:-asia-southeast1}"
SERVICE="${SERVICE:-vitals}"
PROGRAM_ID="${VITALS_PROGRAM_ID:-}"
RPC="${VITALS_RPC:-https://api.devnet.solana.com}"
# Heimdall needs a GPU and cannot run here. Reach the machine that has one — when there is one.
HEIMDALL="${HEIMDALL_API_URL:-}"
# The cloud voice — Vertex, keyless through the metadata server. The public demo runs on this
# (the recorded exception to Heimdall-only: synthetic patient, no PHI, not clinical care); the
# local gateway takes over automatically whenever HEIMDALL_API_URL is reachable.
VERTEX_URL="${VITALS_VERTEX_URL:-}"
VERTEX_MODEL="${VITALS_VERTEX_MODEL:-}"
# The meter: per-address windows and the visible monthly ceiling. Defaults live in the binary;
# these only need setting to change them. The donation link is where the ceiling card points.
MONTHLY="${VITALS_MONTHLY_TURNS:-}"
DONATE="${VITALS_DONATE_URL:-}"

[ -n "$PROGRAM_ID" ] || { echo "set VITALS_PROGRAM_ID (scripts/deploy-devnet.sh prints it)"; exit 1; }
[ -n "$PROJECT" ] || { echo "set VITALS_GCP_PROJECT — this product gets its own project, see above"; exit 1; }

case "$PROJECT" in
  cloud-super-hero*)
    echo "refusing: $PROJECT holds Embla's identified users. Use a project of this product's own." >&2
    exit 1 ;;
esac

# Deploying as whoever gcloud happens to be is how a service ends up running with a backup
# uploader's permissions, or failing halfway with a half-created service to clean up.
WHO="$(gcloud config get-value account 2>/dev/null)"
echo "── as        $WHO"
case "$WHO" in
  "")
    echo "   gcloud has no active account at all. gcloud auth login first." >&2
    exit 1 ;;
  *gserviceaccount.com)
    echo "   that is a service account, not you. gcloud auth login first." >&2
    exit 1 ;;
esac

# A name in the config is not a working credential, and the difference is invisible until
# something spends it. `gcloud config get-value` reads a file on disk and never touches the
# network, so an account whose refresh token has expired — or whose session Google wants
# re-confirmed — reads exactly as healthy as one that works. The first command to notice used
# to be the build, twenty minutes and a few tenths of a dollar later, and it noticed in a shell
# nobody could answer: gcloud's reauth prompt wants a terminal, and a background run has none,
# so it printed "cannot prompt during non-interactive execution" and gave up.
#
# So spend one refresh here, where failing costs a second instead of twenty minutes. The token
# goes to /dev/null; only gcloud's complaint is kept, to hand back verbatim.
if ! AUTH_ERR="$(gcloud auth print-access-token 2>&1 >/dev/null)"; then
  {
    echo "   the credentials for $WHO are on disk but will not refresh:"
    printf '%s\n' "$AUTH_ERR" | sed 's/^/     /'
    echo
    echo "   gcloud auth login   — in a terminal you can answer, then run this again."
    echo "   caught here on purpose: this is the failure that used to surface as a dead build"
    echo "   twenty minutes in, or worse, as a deploy that reported success and shipped nothing."
  } >&2
  exit 1
fi

echo "── project   $PROJECT / $REGION"
echo "── service   $SERVICE"
echo "── rpc       $RPC"

# State goes to Firestore, not a disk: a Cloud Run container has none that survives a request.
# Setting the project is what selects that backend — see store.rs.
# The relay key is read as a file, not an env var, so it is mounted as one below.
ENV="GOOGLE_CLOUD_PROJECT=$PROJECT,VITALS_RPC=$RPC,VITALS_PROGRAM_ID=$PROGRAM_ID,VITALS_SCENARIOS=/app,VITALS_KEYPAIR=/relay/id.json"
[ -n "$HEIMDALL" ] && ENV="$ENV,HEIMDALL_API_URL=$HEIMDALL"
[ -n "$VERTEX_URL" ] && ENV="$ENV,VITALS_VERTEX_URL=$VERTEX_URL"
[ -n "$VERTEX_MODEL" ] && ENV="$ENV,VITALS_VERTEX_MODEL=$VERTEX_MODEL"
[ -n "$MONTHLY" ] && ENV="$ENV,VITALS_MONTHLY_TURNS=$MONTHLY"
[ -n "$DONATE" ] && ENV="$ENV,VITALS_DONATE_URL=$DONATE"
# Where the film lives inside the container — the GCS volume (vitals-academy-clips) is mounted
# once on the service and survives deploys, but --set-env-vars replaces the whole env, so the
# path has to ride along here or a redeploy silently mutes the film.
CLIPS="${VITALS_CLIPS:-/clips/ep1}"
ENV="$ENV,VITALS_CLIPS=$CLIPS"
if [ -z "$HEIMDALL" ] && [ -z "$VERTEX_URL" ]; then
  echo "── voice     none — set VITALS_VERTEX_URL (cloud) or HEIMDALL_API_URL (local); orders still work"
fi

# The Heimdall key is mounted only when there is a gateway to reach. A cloud-only deploy has no
# heimdall-key secret at all, and requiring one would make the optional path mandatory.
SECRETS="/relay/id.json=vitals-relay-key:latest,VITALS_TOKEN=vitals-token:latest"
[ -n "$HEIMDALL" ] && SECRETS="$SECRETS,HEIMDALL_API_KEY=heimdall-key:latest"

# Phases, so CI can put a scanner between the build and the deploy without duplicating any of
# the flags below — a duplicated flag list is a second definition of the deployment, and second
# definitions drift. PHASE=all keeps the one-command behaviour for a human.
PHASE="${PHASE:-all}"

IMAGE="gcr.io/$PROJECT/$SERVICE"

# The build phase and the deploy phase are separate processes, so the digest one produced has to
# be written down for the other to read. Not in the repo — it is not source, and a file in the
# tree is a thing to gitignore forever. TMPDIR is shared across the steps of one CI job, which
# is exactly the span the handoff has to cross.
DIGEST_FILE="${TMPDIR:-/tmp}/vitals-deploy-digest.$PROJECT.$SERVICE"

BUILT_DIGEST=""
if [ "$PHASE" = "build" ] || [ "$PHASE" = "all" ]; then
  # Through cloudbuild.yaml rather than --tag: the Dockerfile needs BuildKit, which the
  # implicit --tag build does not enable.
  #
  # --format asks the build what it actually produced. Asking is the only way to tell "the image
  # this run made" from "whatever the tag happened to point at" — at deploy time those two look
  # identical: same reference, same green output, one of them yesterday's bytes.
  #
  # The fd 3 dance is because `builds submit` streams the build log to stdout (submit_util.py:
  # `out = log.out`), on the same stream the --format value lands on. Capturing stdout plainly
  # would swallow the twenty minutes of build output a human is watching, and swallowing output
  # to gain a check is the trade this script is supposed to stop making. So fd 3 keeps the real
  # stdout, tee puts the log back on it live, and only the last line — the value gcloud prints
  # after the stream closes — is kept.
  exec 3>&1
  BUILT_DIGEST="$(gcloud builds submit --project "$PROJECT" --config cloudbuild.yaml \
    --substitutions "_SERVICE=$SERVICE" \
    --format='value(results.images[0].digest)' . | tee /dev/fd/3 | tail -n 1)"
  exec 3>&-
  # Shape-checked, not just non-empty. If a future gcloud moves the build log or the value onto
  # a different stream, the capture would quietly become a log line, and a log line compared
  # against a digest fails in a way that reads like a stale image rather than like a broken
  # script. Say which one it actually is.
  case "$BUILT_DIGEST" in
    sha256:*) ;;
    "") echo "the build finished but named no image digest — refusing to deploy bytes nothing can identify" >&2
        exit 1 ;;
    *)  echo "the build's last line of output was not a digest:" >&2
        echo "  $BUILT_DIGEST" >&2
        echo "this script reads the digest from --format=value(results.images[0].digest); if gcloud" >&2
        echo "has changed where that lands, fix the capture rather than skipping the check." >&2
        exit 1 ;;
  esac
  echo "── built     $BUILT_DIGEST"
  printf '%s\n' "$BUILT_DIGEST" > "$DIGEST_FILE"
fi

# Spelled as an `if` rather than `[ ... ] && exit 0`: an && list that ends a script hands the
# script its own exit status, and getting that wrong is the bug this file exists to not repeat.
if [ "$PHASE" = "build" ]; then
  exit 0
fi

# Deploying without having built in this process — CI's PHASE=deploy, with a scanner in
# between. Pick up what the build phase wrote down, so the digest check below still has an
# anchor that means "this run's image".
BUILT_SOURCE="the image this run built"
if [ -z "$BUILT_DIGEST" ] && [ -f "$DIGEST_FILE" ]; then
  BUILT_DIGEST="$(cat "$DIGEST_FILE")"
  # Named apart from an in-process build on purpose. This file outlives the run that wrote it,
  # so a PHASE=deploy days later can read a digest from a build nobody remembers; when that
  # mismatches, the message has to say where the expectation came from or it sends someone
  # hunting a stale image that is really a stale note.
  BUILT_SOURCE="the digest the build phase recorded at $DIGEST_FILE"
  echo "── built     $BUILT_DIGEST (recorded by an earlier build phase)"
fi

# One instance, because the anchoring tree is held in memory and two copies would each keep their
# own and overwrite the other's leaves — the leaves their own proofs are built from.
#
# --format again, for the same reason: the revision name comes from the deploy itself, as a
# field. The name is also printed in the prose above it, but prose is a format that changes
# between gcloud releases, and a verification that can be broken by a wording change verifies
# the wording.
REVISION="$(gcloud run deploy "$SERVICE" \
  --project "$PROJECT" --region "$REGION" \
  --image "$IMAGE" \
  --allow-unauthenticated \
  --port 8474 \
  --min-instances 1 --max-instances 1 --concurrency 8 \
  --cpu 1 --memory 512Mi \
  --set-env-vars "$ENV" \
  --set-secrets "$SECRETS" \
  --format='value(status.latestCreatedRevisionName)')"

if [ -z "$REVISION" ]; then
  echo "the deploy returned without naming a revision — there is nothing to verify, so treat it as failed" >&2
  exit 1
fi
echo "── revision  $REVISION"

# Everything above this line is what the deploy *said*. Everything below is reading it back.
# A deploy can land a revision and still leave the old one serving — traffic pinned to a named
# revision, a revision that never went ready — and from the outside that is a green run in front
# of a 404 on the route you just released.
SERVICE_JSON="$(gcloud run services describe "$SERVICE" \
  --project "$PROJECT" --region "$REGION" --format=json)"
REVISION_JSON="$(gcloud run revisions describe "$REVISION" \
  --project "$PROJECT" --region "$REGION" --format=json)"

# When there is no digest from a build in this run, fall back to what the tag resolves to now.
# That is a weaker claim and the check below says so out loud rather than letting a weaker
# claim wear the same word.
EXPECT_DIGEST="$BUILT_DIGEST"
EXPECT_SOURCE="$BUILT_SOURCE"
if [ -z "$EXPECT_DIGEST" ]; then
  EXPECT_DIGEST="$(gcloud container images describe "$IMAGE:latest" \
    --format='value(image_summary.digest)' 2>/dev/null || true)"
  EXPECT_SOURCE="what $IMAGE:latest points at right now"
fi

REVISION="$REVISION" \
EXPECT_DIGEST="$EXPECT_DIGEST" \
EXPECT_SOURCE="$EXPECT_SOURCE" \
SERVICE_JSON="$SERVICE_JSON" \
REVISION_JSON="$REVISION_JSON" \
python3 - <<'PY'
import json, os, sys

rev      = os.environ["REVISION"]
expect   = os.environ["EXPECT_DIGEST"]
source   = os.environ["EXPECT_SOURCE"]
service  = json.loads(os.environ["SERVICE_JSON"])
revision = json.loads(os.environ["REVISION_JSON"])

ok = True
print()

# 1. The revision this deploy created is the revision answering requests.
traffic = service.get("status", {}).get("traffic") or []
split = [(t.get("percent") or 0, t.get("revisionName") or "(unnamed)") for t in traffic]
live = [name for pct, name in split if pct > 0]

if live == [rev]:
    print(f"   serving   {rev}")
else:
    print(f"   NOT SERVING  the deploy created {rev}, but the traffic is:")
    for pct, name in sorted(split, reverse=True):
        print(f"                  {pct:3d}%  {name}")
    print("                a revision that exists but takes no traffic is a deploy that changed nothing")
    print("                for anyone holding the link. If traffic is pinned to an older revision:")
    print("                  gcloud run services update-traffic --to-latest")
    ok = False

# 2. The bytes it runs are the bytes just built. Cloud Run resolves the tag to a digest at
#    deploy time and records it, so this compares what is running against what was produced —
#    not two spellings of the same moving tag.
ref = revision.get("status", {}).get("imageDigest") or ""
got = ref.split("@", 1)[1] if "@" in ref else ref

if not got:
    print("   NO DIGEST    Cloud Run did not record which image bytes this revision runs;")
    print("                there is no way to tell a fresh image from a stale one, so this fails")
    ok = False
elif not expect:
    print(f"   image     {got}")
    print("   UNVERIFIED   nothing to compare against: no build ran in this process and the")
    print(f"                registry would not say what {source} is. Run PHASE=all, or run the")
    print("                build phase in this same checkout so the digest is recorded")
    ok = False
elif got != expect:
    print(f"   STALE IMAGE  the revision runs   {got}")
    print(f"                but {source} is")
    print(f"                                    {expect}")
    print("                the deploy succeeded and shipped something else — which is the one")
    print("                failure that is indistinguishable from success from the outside")
    ok = False
else:
    print(f"   image     {got}")
    print(f"             ({source})")

url = service.get("status", {}).get("url") or ""
if not url:
    print("   NO URL       the service reports no URL, so there is nothing to hand anyone")
    ok = False
else:
    print()
    print(f"   {url}")

sys.exit(0 if ok else 1)
PY
