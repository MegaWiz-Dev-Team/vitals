#!/usr/bin/env bash
# Put the program on devnet and point the cluster at it.
#
# devnet, not testnet. testnet exists to test the validator software; devnet is where applications
# are developed and where a hackathon judge will look. Airdrops work on both, and neither carries
# anything of value.
set -euo pipefail
cd "$(dirname "$0")/.."

CLUSTER="${CLUSTER:-devnet}"
RPC="${RPC:-https://api.$CLUSTER.solana.com}"
KEY="${VITALS_KEYPAIR:-$HOME/.config/solana/id.json}"
# The program keypair IS the program id. Kept outside target/ because `cargo clean` deletes that
# directory, and losing this file changes the id — which strands every record ever anchored.
PROGRAM_KEY="keys/vitals_program-keypair.json"

[ -f "$PROGRAM_KEY" ] || { echo "no program keypair at $PROGRAM_KEY"; exit 1; }
PROGRAM_ID=$(solana address -k "$PROGRAM_KEY")

echo "── cluster    $CLUSTER"
echo "── program    $PROGRAM_ID"
echo "── payer      $(solana address -k "$KEY")"

# 108KB of program needs about 0.78 SOL held as rent, plus fees. The relay also pays rent for every
# player's accounts — roughly 0.011 SOL each — so a demo wants headroom, not the minimum.
NEED=1.5
BAL=$(solana balance -k "$KEY" --url "$RPC" | awk '{print $1}')
echo "── balance    $BAL SOL (want ≥ $NEED)"

if awk "BEGIN{exit !($BAL < $NEED)}"; then
  echo "   requesting an airdrop…"
  solana airdrop 2 -k "$KEY" --url "$RPC" || cat <<'HINT'

   The public faucet rate-limits hard and refuses more often than it works.
   Alternatives, in order of reliability:
     · https://faucet.solana.com — web faucet, sign in with GitHub, far more generous
     · a free RPC from Helius or QuickNode, which usually includes its own faucet
     · any devnet wallet you already funded: solana transfer <addr> 2 --url devnet
HINT
fi

cd crates/vitals-program && cargo build-sbf --arch v3 && cd ../..
solana program deploy target/deploy/vitals_program.so \
  --program-id "$PROGRAM_KEY" --keypair "$KEY" --url "$RPC"

cat <<DONE

   deployed.

   run the server against it:
     export VITALS_RPC=$RPC
     export VITALS_PROGRAM_ID=$PROGRAM_ID
     cargo run -p vitals-web

   or in the cluster:
     VITALS_PROGRAM_ID=$PROGRAM_ID scripts/deploy.sh
     kubectl -n vitals set env deployment/vitals-web VITALS_RPC=$RPC

   explorer:
     https://explorer.solana.com/address/$PROGRAM_ID?cluster=$CLUSTER
DONE
