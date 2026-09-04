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

# ── the payout the deploy intended is the payout that is running ────────────
#
# Same failure as voice, one layer further in: --set-env-vars replaces the whole environment, so
# a revision can come up with payouts off while the shell that deployed it had them on, and
# nothing about that revision looks wrong from outside. The service reports what it actually
# built at boot on /api/authors; this compares that against what was asked for.
#
# Intent is read from the same variable the deploy script reads, so `deploy && verify` in one
# shell checks itself with no second place to state it. Unset means off — the safe reading, and
# the one that makes a payout-enabled service verified from a bare shell say so out loud rather
# than pass quietly.
AUTHORS_URL="${VITALS_AUTHORS_URL:-${VOICE_URL%/api/chain}/api/authors}"
if [ "${SKIP_PAYOUT_CHECK:-}" = "1" ]; then
  echo "── payout    skipped (SKIP_PAYOUT_CHECK=1)"
else
  # The status is read rather than folded into one failure, because 404 here means something
  # specific and actionable: /api/authors arrived with the payout work, so a 404 is a revision
  # older than this script rather than a service that is down. Reporting both as "unreachable"
  # sends someone to check the network for a deploy they simply have not done yet.
  if AUTHORS_HTTP="$(curl -sS -o "$TMP/authors.json" -w '%{http_code}' "$AUTHORS_URL" 2>"$TMP/authors.err")"; then
    case "$AUTHORS_HTTP" in
      # 000 is what a non-HTTP URL reports — a file:// document, which is how this block's
      # branches are exercised without standing up a service.
      200|000) ;;
      404)
        echo "   NO ENDPOINT  $AUTHORS_URL answered 404." >&2
        echo "                The running revision is older than this script: /api/authors came" >&2
        echo "                in with the payout work. Deploy this build, then verify it." >&2
        exit 1 ;;
      *)
        echo "   UNREACHABLE  $AUTHORS_URL answered $AUTHORS_HTTP" >&2
        exit 1 ;;
    esac
  else
    echo "   UNREACHABLE  $AUTHORS_URL" >&2
    sed 's/^/                /' "$TMP/authors.err" >&2
    exit 1
  fi
  AUTHORS_JSON="$(cat "$TMP/authors.json")"
  [ -n "$AUTHORS_JSON" ] || { echo "   EMPTY  $AUTHORS_URL answered $AUTHORS_HTTP with no body" >&2; exit 1; }
  WANT_RATE="${VITALS_PAYOUT_LAMPORTS:-0}" AUTHORS_JSON="$AUTHORS_JSON" python3 <<'PY'
import json, os, sys

d = json.loads(os.environ["AUTHORS_JSON"])
p = d.get("payouts") or {}
on = bool(p.get("on"))

want_raw = os.environ["WANT_RATE"].strip()
if not want_raw.isdigit():
    print(f'   BAD INTENT  VITALS_PAYOUT_LAMPORTS is "{want_raw}", which is not a number of')
    print("               lamports. The deploy refuses this too; there is no intent to check")
    print("               against until it is a whole number, or 0 for off.")
    sys.exit(1)
want_rate = int(want_raw)
want_on = want_rate > 0

if on != want_on:
    asked = f"on at {want_rate} lamports" if want_on else "off"
    got = f"on at {p.get('lamports_per_proven_replay')} lamports" if on else "off"
    print(f"   MISMATCH  the deploy intended payouts {asked}; the service reports {got}")
    if want_on:
        print("             the environment did not reach the revision. --set-env-vars replaces")
        print("             the whole environment, so every payout variable has to ride along on")
        print("             the deploy that sets any of them — see the `── payout` line the")
        print("             deploy script prints, which says which of the two it shipped.")
    else:
        print("             a revision is paying out that this shell did not ask to pay out.")
        print("             If that is intended, say so here too: VITALS_PAYOUT_LAMPORTS=<rate>")
    sys.exit(1)

if not on:
    print("   payout    off — runs are proven, nobody is paid, as intended")
    sys.exit(0)

rate = p.get("lamports_per_proven_replay")
if rate != want_rate:
    print(f"   MISMATCH  payouts are on, but at {rate} lamports per proven replay, not the")
    print(f"             {want_rate} this shell asked for. The rate is what a learner is paid;")
    print("             a revision paying a different one is not a smaller version of the same")
    print("             deploy.")
    sys.exit(1)

wallet = p.get("wallet") or "(none reported)"
print(f"   payout    on · {rate} lamports per proven replay · wallet {wallet}")
PY
  echo "── the payout is the one the deploy asked for"
fi
