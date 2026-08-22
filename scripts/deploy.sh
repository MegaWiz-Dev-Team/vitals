#!/usr/bin/env bash
# Build the image and roll it into the local cluster.
#
# The secrets are created from files you already have rather than committed: the relay keypair is
# a real key that pays real fees, and the bearer token is what stops a public bind from signing
# for strangers.
set -euo pipefail
cd "$(dirname "$0")/.."

NS=vitals
RELAY="${VITALS_KEYPAIR:-$HOME/.config/solana/id.json}"
PROGRAM_ID="${VITALS_PROGRAM_ID:-}"

[ -f "$RELAY" ] || { echo "no relay keypair at $RELAY — set VITALS_KEYPAIR"; exit 1; }
[ -n "$PROGRAM_ID" ] || { echo "set VITALS_PROGRAM_ID to the deployed program"; exit 1; }

echo "── building vitals-web:latest"
docker build -t vitals-web:latest .

echo "── namespace and config"
kubectl apply -f k8s/vitals.yaml >/dev/null

kubectl -n "$NS" create configmap vitals-config \
    --from-literal=program-id="$PROGRAM_ID" \
    --dry-run=client -o yaml | kubectl apply -f - >/dev/null

# Generated once and kept. Rotating it every deploy would log every open tab out mid-case.
if ! kubectl -n "$NS" get secret vitals-secrets >/dev/null 2>&1; then
    kubectl -n "$NS" create secret generic vitals-secrets \
        --from-literal=token="$(openssl rand -hex 32)" >/dev/null
    echo "   created a new bearer token"
fi

kubectl -n "$NS" create secret generic vitals-relay \
    --from-file=id.json="$RELAY" \
    --dry-run=client -o yaml | kubectl apply -f - >/dev/null

echo "── rolling out"
kubectl -n "$NS" rollout restart deployment/vitals-web >/dev/null
kubectl -n "$NS" rollout status deployment/vitals-web --timeout=180s

cat <<DONE

   vitals is up inside the cluster, and only inside it — there is no Ingress
   because a publicly reachable demo is public disclosure and the patent has
   to be filed first.

   reach it:   kubectl -n $NS port-forward svc/vitals-web 8474:8474
   token:      kubectl -n $NS get secret vitals-secrets -o jsonpath='{.data.token}' | base64 -d
DONE
