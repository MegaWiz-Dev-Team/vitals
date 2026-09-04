#!/usr/bin/env bash
# Check that the program running on a cluster is the program in this working tree.
#
# The repository asks to be taken as "a protocol with one reference client". Publishing source is
# only half of that: without a way to check the deployed bytecode against it, "open source" is a
# claim about a repository rather than about the thing actually answering transactions. This is
# the check.
#
# What it proves, exactly: the bytecode on chain is byte-for-byte the artefact this machine
# builds. What it does not prove is that *your* machine would build the same artefact — that needs
# a pinned toolchain in a container, which is what `solana-verify` exists for and what should
# replace this script the moment it can be installed. Until then this catches the failure that
# actually happens: a deploy that silently lagged behind the source.
set -euo pipefail
cd "$(dirname "$0")/.."

# The id, or the key that is the id. No in-repo fallback: the keypair lives outside this
# repository and its path is given.
PROGRAM_ID="${VITALS_PROGRAM_ID:-}"
if [ -z "$PROGRAM_ID" ] && [ -n "${VITALS_PROGRAM_KEY:-}" ]; then
  PROGRAM_ID=$(solana address -k "$VITALS_PROGRAM_KEY" 2>/dev/null || true)
fi
URL="${VITALS_RPC:-http://127.0.0.1:8899}"
SO="target/deploy/vitals_program.so"

[ -n "$PROGRAM_ID" ] || { echo "set VITALS_PROGRAM_ID, or VITALS_PROGRAM_KEY pointing at the program keypair outside this repository"; exit 1; }
[ -f "$SO" ] || { echo "no $SO — build it first: cd crates/vitals-program && cargo build-sbf --arch v3"; exit 1; }

echo "── program  $PROGRAM_ID"
echo "── cluster  $URL"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
solana program dump "$PROGRAM_ID" "$TMP/onchain.so" --url "$URL" >/dev/null

# The on-chain account is padded with zeros so the program can grow on upgrade. Comparing whole
# files would report a mismatch on every deploy; what has to match is the prefix, and the padding
# has to be nothing but zeros — a non-zero tail would mean the account holds something this build
# does not account for.
python3 - "$TMP/onchain.so" "$SO" <<'PY'
import sys, hashlib
chain = open(sys.argv[1], 'rb').read()
local = open(sys.argv[2], 'rb').read()
n = len(local)

if len(chain) < n:
    print(f"   MISMATCH  on chain is {len(chain)} bytes, shorter than the {n}-byte build")
    sys.exit(1)
if chain[:n] != local:
    for i, (a, b) in enumerate(zip(chain, local)):
        if a != b:
            print(f"   MISMATCH  first difference at byte {i}")
            break
    sys.exit(1)
if set(chain[n:]) - {0}:
    print("   MISMATCH  the tail past the program is not zero padding")
    sys.exit(1)

print(f"   match     {hashlib.sha256(local).hexdigest()}")
print(f"             {n} bytes, plus {len(chain)-n} bytes of zero padding on chain")
PY

echo "── the deployed program is this build"

# ── the patients can speak ──────────────────────────────────────────────────
#
# The program being right says nothing about whether the bay it serves has a voice. Two things
# have muted it before and neither raised an error: a deploy from a shell with no
# VITALS_VERTEX_URL (revision 43), and personas that were never copied into the image at all —
# every OSCE station's patient was silent from the day the stations got voices.
#
# So this asks the running service, and counts against the tree rather than a number typed here:
# one persona file per voiced case, plus ep1, which reads its own scenario.
VOICE_URL="${VITALS_VOICE_URL:-https://devnet.vitals.academy/api/chain}"
if [ "${SKIP_VOICE_CHECK:-}" = "1" ]; then
  echo "── voice     skipped (SKIP_VOICE_CHECK=1)"
else
  EXPECTED=$(( $(ls demo/personas/*.json 2>/dev/null | wc -l | tr -d ' ') + 1 ))
  # The document goes in the environment, not down the pipe: a heredoc script and piped data
  # both want stdin, and python reads whichever arrives — which meant it parsed the JSON as its
  # own source and failed with a SyntaxError that looked nothing like a voice problem.
  CHAIN_JSON="$(curl -fsS "$VOICE_URL")" || { echo "   UNREACHABLE  $VOICE_URL" >&2; exit 1; }
  EXPECTED="$EXPECTED" CHAIN_JSON="$CHAIN_JSON" python3 <<'PY'
import json, os, sys
d = json.loads(os.environ["CHAIN_JSON"])
want = int(os.environ["EXPECTED"])
voiced = d.get("voiced") or []
if not d.get("voice"):
    print("   MUTE      the service reports no gateway at all — the patients cannot speak")
    sys.exit(1)
if len(voiced) != want:
    print(f"   MISMATCH  {len(voiced)} case(s) can speak, expected {want}")
    print(f"             voiced: {', '.join(voiced) or 'none'}")
    print("             a persona in demo/personas that is not in the image is the usual cause —")
    print("             check the Dockerfile copies it, and see tests/image.rs")
    sys.exit(1)
print(f"   voice     {len(voiced)} of {want} cases can speak")
PY
  echo "── the patients can speak"
fi
