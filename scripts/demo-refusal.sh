#!/bin/bash
# A server that cannot prove what it already anchored, refusing to anchor more.
#
# Everything else this project shows is a system working. This is a system declining to work,
# with nobody watching and nothing forcing it — which is the only kind of evidence that means
# anything when the claim is "you do not have to trust us".
#
# Six runs are anchored against a real program on a real validator. One leaf is then removed from
# the server's own copy of the tree, the way a rolled-back or half-restored store would lose one.
# The server is asked for a seventh anchor. It declines, prints the chain's count beside its own,
# and says why.
#
# Nothing here is staged: the runs are played through the same endpoints a learner uses, the
# transactions are signed and submitted, and the refusal comes from `reconcile_leaves` in
# `crates/vitals-web/src/main.rs` rather than from anything this script does.
#
#   scripts/demo-refusal.sh              a validator you already have
#   scripts/demo-refusal.sh --start      start one, deploy, run, tear it down

set -uo pipefail
cd "$(dirname "$0")/.."

RPC="${VITALS_RPC:-http://127.0.0.1:8899}"
LEDGER="${LEDGER_DIR:-$(mktemp -d)/ledger}"
SBF="${SBF_TARGET:-$(mktemp -d)/sbf}"
STARTED_VALIDATOR=

cleanup() {
  [ -n "$STARTED_VALIDATOR" ] && { pkill -f "solana-test-validator --reset .*$LEDGER" 2>/dev/null; }
  return 0
}
trap cleanup EXIT

if [ "${1:-}" = "--start" ]; then
  echo "── starting a validator"
  solana-test-validator --reset --quiet --ledger "$LEDGER" >/dev/null 2>&1 &
  STARTED_VALIDATOR=1
  for _ in $(seq 1 40); do solana cluster-version -u "$RPC" >/dev/null 2>&1 && break; sleep 1; done
  solana cluster-version -u "$RPC" >/dev/null 2>&1 || { echo "no validator at $RPC" >&2; exit 1; }
  solana airdrop 100 -u "$RPC" >/dev/null 2>&1
  echo "── building and deploying the program"
  ( cd crates/vitals-program && CARGO_TARGET_DIR="$SBF" cargo build-sbf --arch v3 >/dev/null 2>&1 )
  PROGRAM_ID=$(solana program deploy "$SBF/deploy/vitals_program.so" -u "$RPC" \
    --program-id "$SBF/deploy/vitals_program-keypair.json" 2>/dev/null | awk '/Program Id/{print $3}')
  [ -n "$PROGRAM_ID" ] || { echo "deploy failed" >&2; exit 1; }
  export VITALS_PROGRAM_ID="$PROGRAM_ID"
fi

: "${VITALS_PROGRAM_ID:?set VITALS_PROGRAM_ID, or pass --start to bring up a validator}"
solana cluster-version -u "$RPC" >/dev/null 2>&1 || { echo "no validator at $RPC" >&2; exit 1; }

echo "── program  $VITALS_PROGRAM_ID"
echo "── cluster  $RPC"

# The demonstration is a test, so it is the same code the gates run and cannot drift into a
# performance that no longer matches the server.
VITALS_RPC="$RPC" cargo test -p vitals-web --test chain_flow \
  demo_a_server_that_cannot_prove -- --ignored --nocapture 2>&1 \
  | sed -n '/@@DEMO-BEGIN@@/,/@@DEMO-END@@/p' | grep -v '@@DEMO-'
