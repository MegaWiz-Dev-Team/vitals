#!/usr/bin/env bash
# Put the server on Cloud Run, where embla-cloud already lives.
#
# The whole product is one Rust binary — page, API and relay together — so there is no separate
# frontend to host. That is a consequence of the relay paying fees: a static host cannot hold a
# signing key, and holding one is what lets a medical student play without ever touching crypto.
set -euo pipefail
cd "$(dirname "$0")/.."

PROJECT="${VITALS_GCP_PROJECT:-cloud-super-hero-dev}"
REGION="${REGION:-asia-southeast1}"
SERVICE="${SERVICE:-vitals}"
PROGRAM_ID="${VITALS_PROGRAM_ID:-}"
RPC="${VITALS_RPC:-https://api.devnet.solana.com}"
# Heimdall needs a GPU and cannot run here. Reach the machine that has one.
HEIMDALL="${HEIMDALL_API_URL:-}"

[ -n "$PROGRAM_ID" ] || { echo "set VITALS_PROGRAM_ID (scripts/deploy-devnet.sh prints it)"; exit 1; }

echo "── project   $PROJECT / $REGION"
echo "── service   $SERVICE"
echo "── rpc       $RPC"

# State goes to Firestore, not a disk: a Cloud Run container has none that survives a request.
# Setting the project is what selects that backend — see store.rs.
# The relay key is read as a file, not an env var, so it is mounted as one below.
ENV="GOOGLE_CLOUD_PROJECT=$PROJECT,VITALS_RPC=$RPC,VITALS_PROGRAM_ID=$PROGRAM_ID,VITALS_SCENARIOS=/app,VITALS_KEYPAIR=/relay/id.json"
[ -n "$HEIMDALL" ] && ENV="$ENV,HEIMDALL_API_URL=$HEIMDALL"

gcloud builds submit --project "$PROJECT" --tag "gcr.io/$PROJECT/$SERVICE" .

# One instance, because the anchoring tree is held in memory and two copies would each keep their
# own and overwrite the other's leaves — the leaves their own proofs are built from.
gcloud run deploy "$SERVICE" \
  --project "$PROJECT" --region "$REGION" \
  --image "gcr.io/$PROJECT/$SERVICE" \
  --allow-unauthenticated \
  --min-instances 1 --max-instances 1 --concurrency 8 \
  --cpu 1 --memory 512Mi \
  --set-env-vars "$ENV" \
  --set-secrets "/relay/id.json=vitals-relay-key:latest,VITALS_TOKEN=vitals-token:latest,HEIMDALL_API_KEY=heimdall-key:latest"

echo
echo "   $(gcloud run services describe "$SERVICE" --project "$PROJECT" --region "$REGION" --format='value(status.url)')"
