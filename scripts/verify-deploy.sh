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

PROGRAM_ID="${VITALS_PROGRAM_ID:-$(solana address -k keys/vitals_program-keypair.json 2>/dev/null || true)}"
URL="${VITALS_RPC:-http://127.0.0.1:8899}"
SO="target/deploy/vitals_program.so"

[ -n "$PROGRAM_ID" ] || { echo "set VITALS_PROGRAM_ID, or keep keys/vitals_program-keypair.json"; exit 1; }
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
