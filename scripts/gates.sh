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

# One gate run on this machine at a time. Two sessions ran the workspace tests at once on
# 25 Sep 2026 and the contention turned one RED with no failing test — a pre-flight on one side
# cannot stop the other side starting, only a lock both sides take can. gates.sh re-runs itself
# through scripts/gate-lock.sh once; under the lock GATE_LOCK_HELD is set and it proceeds.
#
# Both copies of this script must take it: cwf/ops holds the original and this is cwf/ward's, and a
# lock one side takes serializes nothing.
if [ "${GATE_LOCK_HELD:-}" != 1 ]; then
  exec bash "$(dirname "$0")/gate-lock.sh" bash "$0" "$@"
fi

FAIL=0
# Every gate's output is kept, and a FAIL shows the end of its own. Three reds on 25 Sep 2026
# recorded a gate's name and nothing else — both streams went to /dev/null — so not one of them
# named the failing test, and each cost a diagnosis it should have carried in itself.
GATE_LOGS="${GATE_LOGS:-target/gates}"; mkdir -p "$GATE_LOGS"
gate() {
  local name="$1"; shift
  local log; log="$GATE_LOGS/$(printf '%s' "$name" | tr -c 'A-Za-z0-9\n' '-' | sed 's/-*$//').log"
  if "$@" >"$log" 2>&1; then
    printf '  \033[32mpass\033[0m  %s\n' "$name"
  else
    printf '  \033[31mFAIL\033[0m  %s — output kept in %s; its last lines:\n' "$name" "$log"
    grep -n 'FAILED\|panicked\|^failures:\|error\[\|^error:\|timed out' "$log" | head -12 | sed 's/^/        /'
    tail -n 25 "$log" | sed 's/^/        │ /'
    FAIL=1
  fi
}

echo "── gates ──"
# Clippy and rustc write different artifacts, and in one target dir each forces the other to
# rebuild — measured 22–23 Sep at about 45 minutes of wall clock over one night's four runs. A
# target dir of clippy's own is check-mode only: 902 MB for the whole workspace (481 crates,
# 67 s from empty, measured 23 Sep), not a second copy of the 23 GB test tree.
gate "clippy -D warnings"  env CARGO_TARGET_DIR=target/clippy cargo clippy --workspace --all-targets --offline -- -D warnings
# The test threads are capped. This box carries a standing load of about 6 on 12 cores (the
# cluster's backends, the window server, the IDE), and `cargo test` defaults to a thread per
# core: twelve threads on five spare cores is contention on every run, lock or no lock, and
# contention fabricates reds — never greens. Six is half the cores; name another with
# GATE_TEST_THREADS on a box that is different.
gate "workspace tests"     cargo test --workspace --offline -- --test-threads="${GATE_TEST_THREADS:-6}"

# The globe's own arithmetic — country lookup, the per-country counts, the difficulty filter —
# runs in node against the page it is extracted from, so a change to the page that breaks the
# ward's front door fails here rather than in front of a judge. Skipped with a line, never
# silently, on a machine without node.
if command -v node >/dev/null 2>&1; then
  gate "globe logic" node crates/vitals-web/tests/world/globe_logic.mjs crates/vitals-web/static/world/index.html
  gate "deploy script"  bash scripts/deploy-cloudrun-test.sh
  gate "door script"    bash scripts/ward-door-test.sh
  gate "opening check"  bash scripts/opening-check-test.sh
  gate "cases logic"    node crates/vitals-web/tests/world/cases_logic.mjs crates/vitals-web/static/world/review.html
  # The lock's own harness — no cargo, so it costs nothing and proves the thing that stops two
  # gate runs colliding still works.
  gate "gate lock"      bash scripts/gate-lock-test.sh
  # A generated sentence about a patient takes its pronoun from the board, never from a guess. The
  # server's plain-words gate never reached the page, so this reads the page itself and fails on any
  # interpolated template carrying a bare she/he/her/him/his. From cwf/ops, where it was written;
  # it belongs here, with the pronoun code it guards.
  gate "patient words"  node crates/vitals-web/tests/world/patient_words.mjs crates/vitals-web/static/world/index.html crates/vitals-web/static/bay.js
else
  printf '  \033[33mskip\033[0m  globe logic — node not installed (CI still runs it)\n'
fi

# The factory that ships against this ward runs from ~/.vitals/bin/vitals-factory, a link named for
# its commit. If that commit is not in this branch, the factory crate these gates just checked is
# not the crate that runs — the ward's copy fell eight commits behind, unnoticed, 20–23 Sep 2026,
# while a fixture in its tests went on asserting a shape the ward no longer served. Blind to
# cherry-picks on purpose: a copy under another sha is exactly the divergence this is for.
if [ -L "$HOME/.vitals/bin/vitals-factory" ]; then
  SHIPS="$(readlink "$HOME/.vitals/bin/vitals-factory" | sed 's/^vitals-factory-//')"
  gate "factory that ships ($SHIPS) is in this branch" git merge-base --is-ancestor "$SHIPS" HEAD
else
  printf '  \033[33mskip\033[0m  factory that ships — no ~/.vitals/bin/vitals-factory on this machine (the mini has it)\n'
fi

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
    # SKIP_PAYOUT_CHECK because gates run *before* a deploy, always. The payout check reads the
    # running service and compares it against what this shell asked for, which is a question
    # about a revision — and the only revision standing when gates run is the previous one,
    # whose payout state says nothing about this build. It runs where it means something:
    # `deploy-cloudrun.sh && verify-deploy.sh` in one shell. The program check above is a
    # different thing and does belong here: it compares the local validator's bytes to this tree.
    gate "verify deployed bytecode" env VITALS_RPC=http://127.0.0.1:8899 SKIP_PAYOUT_CHECK=1 \
      scripts/verify-deploy.sh
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

# An optional last reading: the same review CI runs, before the change leaves the machine. Off by
# default because it makes a network call and needs the cluster; on with AI_REVIEW=1. It never
# fails the gates — a reviewer's opinion is advice, not a gate — it just prints.
if [ "${AI_REVIEW:-0}" = "1" ]; then
  echo "── ai pre-review (Gemini, local) ──"
  scripts/ai-review-local.sh
fi

[ "$FAIL" = 0 ] && echo "── all green ──" || { echo "── RED ──"; exit 1; }
