#!/usr/bin/env bash
# A second reading of the working changes, before they ever leave this machine — the local mirror
# of the Gemini PR review in CI, and the same auth as CI: none stored.
#
# It uses the gcloud login already on this machine to mint a short-lived Vertex token, exactly as
# CI mints one from GitHub's OIDC. No API key is read, copied, or held — which sidesteps the
# question of *which* cluster key to borrow, and its real answer: neither. The cluster's keys
# belong to the services that own them (bifrost's serves prod; syn's belongs to a PHI system), and
# a code-review convenience has no business spending either one's quota or muddying its audit
# trail. A token from your own login is attributed to you and expires on its own.
#
# Only the diff is sent, never the tree, and the review prints to the terminal — advice for the
# person about to commit, not a record.
set -uo pipefail
cd "$(dirname "$0")/.."

DIFF=$(git diff HEAD)
[ -n "$DIFF" ] || { echo "  nothing changed — nothing to review"; exit 0; }

TOKEN=$(gcloud auth print-access-token 2>/dev/null)
if [ -z "$TOKEN" ]; then
  echo "  ai-review skipped: no gcloud login (run: gcloud auth login)"
  exit 0
fi
PROJECT="${VITALS_GCP_PROJECT:-vitals-academy}"
LOCATION="${VERTEX_LOCATION:-global}"

INV=$(cat docs/INVARIANTS.md 2>/dev/null)

# Token and payloads go in through the environment, never as arguments where ps and shell history
# would capture them.
REVIEW_TOKEN="$TOKEN" REVIEW_PROJECT="$PROJECT" REVIEW_LOCATION="$LOCATION" \
REVIEW_DIFF="$DIFF" REVIEW_INV="$INV" python3 - <<'PY'
import os, json, urllib.request, urllib.error

prompt = f"""You are the security reviewer for a Solana program whose product is trust: every
anchored leaf is a claim someone will verify years from now.

These are the invariants the program promises to keep:

{os.environ['REVIEW_INV'][:16000]}

Review this uncommitted diff. For every write path it touches, ask: who is allowed to write here,
and does the code check the actor or merely the shape? The two real bugs in this repository's
history were checks that existed but bound the wrong thing — a tree PDA from a guessable id, a
storage key that named nobody.

Flag, with file and line:
- any value a caller supplies that the program then treats as proven
- any new PDA or storage key whose seeds do not name an owner
- any change moving an existing byte of the AttemptRecord encoding (offsets are frozen; only
  appending under a new version tag is legal)
- arithmetic on counters or scores outside the existing bounds reasoning
- any new #[ignore] test, silenced lint, or weakened assertion
- anything leaking hidden/rubric content to a public surface
- an invariant the change now relies on that is missing from INVARIANTS.md

Be brief and concrete. If nothing matters, say exactly: nothing security-relevant.

DIFF:
{os.environ['REVIEW_DIFF'][:60000]}
"""

proj, loc = os.environ["REVIEW_PROJECT"], os.environ["REVIEW_LOCATION"]
host = "aiplatform.googleapis.com" if loc == "global" else f"{loc}-aiplatform.googleapis.com"
url = (f"https://{host}/v1/projects/{proj}/locations/{loc}"
       f"/publishers/google/models/gemini-2.5-flash:generateContent")
req = urllib.request.Request(
    url,
    data=json.dumps({"contents": [{"role": "user", "parts": [{"text": prompt}]}]}).encode(),
    headers={"Content-Type": "application/json",
             "Authorization": "Bearer " + os.environ["REVIEW_TOKEN"]},
)
try:
    with urllib.request.urlopen(req, timeout=60) as r:
        d = json.load(r)
    print("\n" + d["candidates"][0]["content"]["parts"][0]["text"].strip() + "\n")
except urllib.error.HTTPError as e:
    print(f"  ai-review error: HTTP {e.code} — {e.read()[:200].decode(errors='replace')}")
except Exception as e:
    print(f"  ai-review error: {e}")
PY
