#!/usr/bin/env bash
# The names the service answers to, declared once so they can be re-applied rather than
# remembered. Three names, one Cloud Run service: the apex (the front door), devnet (the game)
# and www (a spelling of the apex the server 301s home). Re-runnable: anything that already
# exists is left alone, so this is also the audit of what is there.
#
# The DNS zone and the registration are not created here — both predate this script and a
# registrar is not something to re-run into existence. What is checked first is the registrar's
# view, because of 2026-09-08: Cloud Domains suspends a registration whose contact email is
# never verified (15 days after registration, and after every contact change), the registry
# then pulls the delegation, and the symptom is NXDOMAIN everywhere while Cloud Run, the cert
# and this zone all stay green. `state`/`issues` below say so in one line; DNS never will.
set -euo pipefail

PROJECT="${VITALS_GCP_PROJECT:-vitals-academy}"
REGION="${REGION:-asia-southeast1}"
SERVICE="${SERVICE:-vitals}"
ZONE="${VITALS_DNS_ZONE:-vitals-academy}"
APEX="vitals.academy"

echo "── registrar  $(gcloud domains registrations describe "$APEX" --project "$PROJECT" --format='value(state,issues)')"
echo "              (SUSPENDED / UNVERIFIED_EMAIL = resend the verification mail from the Cloud Domains page; nothing below will help)"

# www rides Google's front end by CNAME. The apex cannot (a CNAME at the zone apex is not a
# thing) and keeps the A/AAAA set the mapping handed out when it was created.
if gcloud dns record-sets describe "www.$APEX." --type CNAME --zone "$ZONE" --project "$PROJECT" >/dev/null 2>&1; then
  echo "── dns        www.$APEX CNAME present"
else
  gcloud dns record-sets create "www.$APEX." --type CNAME --ttl 300 \
    --rrdatas ghs.googlehosted.com. --zone "$ZONE" --project "$PROJECT"
  echo "── dns        www.$APEX CNAME created"
fi

for host in "$APEX" "devnet.$APEX" "www.$APEX"; do
  if gcloud beta run domain-mappings describe --domain "$host" --region "$REGION" --project "$PROJECT" >/dev/null 2>&1; then
    echo "── mapping    $host present"
  else
    gcloud beta run domain-mappings create --service "$SERVICE" --domain "$host" \
      --region "$REGION" --project "$PROJECT"
    echo "── mapping    $host created (the managed certificate follows once the name resolves)"
  fi
done

gcloud beta run domain-mappings list --region "$REGION" --project "$PROJECT" \
  --format='table(metadata.name,spec.routeName,status.conditions[0].status,status.conditions[0].message)'
