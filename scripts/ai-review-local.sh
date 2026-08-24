#!/usr/bin/env bash
# A second reading of the working changes, before they ever leave this machine — the local mirror
# of the Gemini PR review in CI.
#
# The key never touches disk and never touches a command line. It is read from the cluster secret
# into one shell variable, handed to the reviewer through the environment, and gone when the
# process exits. Nothing is uploaded but the diff and the invariants, and the review prints to the
# terminal — it is advice for the person about to commit, not a record.
#
# Two deliberate refusals:
#   - it reads syn-api-secrets, not asgard-secrets: the latter is the key bifrost serves prod with,
#     and a local tool must not share a fate with a running service.
#   - it sends a DIFF, never the whole tree. The hidden rubric content this product guards lives in
#     embla-cases, not here, but sending only the change keeps the habit honest.
set -uo pipefail
cd "$(dirname "$0")/.."

DIFF=$(git diff HEAD)
[ -n "$DIFF" ] || { echo "  nothing changed — nothing to review"; exit 0; }

KEY=$(kubectl get secret -n asgard syn-api-secrets \
        -o jsonpath='{.data.GEMINI_API_KEY}' 2>/dev/null | base64 -d)
if [ -z "$KEY" ]; then
  echo "  ai-review skipped: syn-api-secrets/GEMINI_API_KEY not reachable (need the cluster)"
  exit 0
fi

INV=$(cat docs/INVARIANTS.md 2>/dev/null)

# The key goes in through the environment, the payloads through stdin — never as arguments, where
# they would show up in `ps` and the shell history of anyone sharing this box.
GEMINI_KEY="$KEY" REVIEW_DIFF="$DIFF" REVIEW_INV="$INV" python3 - <<'PY'
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

req = urllib.request.Request(
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent",
    data=json.dumps({"contents": [{"parts": [{"text": prompt}]}]}).encode(),
    headers={"Content-Type": "application/json", "x-goog-api-key": os.environ["GEMINI_KEY"]},
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
