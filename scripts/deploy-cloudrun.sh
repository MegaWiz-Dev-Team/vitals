#!/usr/bin/env bash
# Put the server on Cloud Run, in a project of its own.
#
# The whole product is one Rust binary — page, API and relay together — so there is no separate
# frontend to host. That is a consequence of the relay paying fees: a static host cannot hold a
# signing key, and holding one is what lets a medical student play without ever touching crypto.
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
  *gserviceaccount.com)
    echo "   that is a service account, not you. gcloud auth login first." >&2
    exit 1 ;;
esac

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

if [ "$PHASE" = "build" ] || [ "$PHASE" = "all" ]; then
  # Through cloudbuild.yaml rather than --tag: the Dockerfile needs BuildKit, which the
  # implicit --tag build does not enable.
  gcloud builds submit --project "$PROJECT" --config cloudbuild.yaml --substitutions "_SERVICE=$SERVICE" .
fi
[ "$PHASE" = "build" ] && exit 0

# One instance, because the anchoring tree is held in memory and two copies would each keep their
# own and overwrite the other's leaves — the leaves their own proofs are built from.
gcloud run deploy "$SERVICE" \
  --project "$PROJECT" --region "$REGION" \
  --image "gcr.io/$PROJECT/$SERVICE" \
  --allow-unauthenticated \
  --port 8474 \
  --min-instances 1 --max-instances 1 --concurrency 8 \
  --cpu 1 --memory 512Mi \
  --set-env-vars "$ENV" \
  --set-secrets "$SECRETS"

echo
echo "   $(gcloud run services describe "$SERVICE" --project "$PROJECT" --region "$REGION" --format='value(status.url)')"
