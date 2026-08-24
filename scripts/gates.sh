#!/usr/bin/env bash
# Every gate CI runs, runnable in one command before a commit.
#
# This script is the AI half of the pipeline in practice: the agent that writes the code runs it
# on every iteration, so by the time GitHub sees a push, everything here has already passed once
# on the machine that made the change. CI then repeats it on neutral hardware, and the PR review
# workflow reads the change with no memory of having written it.
#
# Chain gates run only when a local validator is up — start one with:
#   solana-test-validator --reset --quiet &
set -uo pipefail
cd "$(dirname "$0")/.."

FAIL=0
gate() {
  local name="$1"; shift
  if "$@" >/dev/null 2>&1; then
    printf '  \033[32mpass\033[0m  %s\n' "$name"
  else
    printf '  \033[31mFAIL\033[0m  %s\n' "$name"
    FAIL=1
  fi
}

echo "── gates ──"
gate "clippy -D warnings"  cargo clippy --workspace --all-targets --offline -- -D warnings
gate "workspace tests"     cargo test --workspace --offline

if command -v gitleaks >/dev/null 2>&1; then
  gate "gitleaks (history)" gitleaks git --no-banner .
else
  printf '  \033[33mskip\033[0m  gitleaks — not installed (CI still runs it)\n'
fi
if command -v cargo-deny >/dev/null 2>&1; then
  gate "cargo-deny"        cargo deny check advisories licenses sources bans
else
  printf '  \033[33mskip\033[0m  cargo-deny — not installed (CI still runs it)\n'
fi

if solana -u localhost cluster-version >/dev/null 2>&1; then
  if [ -n "${VITALS_PROGRAM_ID:-}" ]; then
    gate "verify deployed bytecode" env VITALS_RPC=http://127.0.0.1:8899 scripts/verify-deploy.sh
    gate "chain tests (serial)" env VITALS_RPC=http://127.0.0.1:8899 \
      cargo test -p vitals-web --test chain_flow --offline -- --ignored --test-threads=1
    gate "cli season"           env VITALS_RPC=http://127.0.0.1:8899 \
      cargo test -p vitals-cli --test driver --offline -- --ignored
  else
    printf '  \033[33mskip\033[0m  chain — set VITALS_PROGRAM_ID\n'
  fi
else
  printf '  \033[33mskip\033[0m  chain — no local validator\n'
fi

[ "$FAIL" = 0 ] && echo "── all green ──" || { echo "── RED ──"; exit 1; }
