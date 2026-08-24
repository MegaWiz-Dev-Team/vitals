# The pipeline

What runs where, what a human must still sign, and why each boundary sits where it does.

## The loop

```
agent writes code
  → scripts/gates.sh          every gate, locally, before every commit
    → push → CI               the same gates on neutral hardware, plus supply-chain
      → PR → ai-review        a second AI reading the change against INVARIANTS.md,
                              with no memory of having written it
        → cd-demo (button)    human-approved deploy of the demo, keyless auth,
                              image scanned before it serves anyone
```

The AI in this pipeline is not a bolt-on scanner. The agent that writes the code runs the gates
each iteration; the review workflow is a *different model from a different vendor* (Gemini on
Vertex) reading the diff cold, told exactly what this program has promised
(`docs/INVARIANTS.md`) — because both real authorization bugs here were found by asking *who is
allowed to write here*, not by pattern-matching. Cross-vendor is deliberate: a reviewer that
shares the writer's model shares the writer's blind spots.

## CI — `.github/workflows/ci.yml`, on every push and PR

| job | gate |
|---|---|
| `test` | `clippy -D warnings` (no blanket allows — the two exceptions are named, with reasons) · 183 workspace tests |
| `chain` | build the program, start a **local validator in the runner**, deploy a **throwaway id**, `verify-deploy.sh` byte-for-byte, then the chain tests that had never run anywhere before 24 Aug |
| `supply-chain` | `cargo-deny`: advisories (exact-id ignores with dated reasons — a *new* advisory still fails), licences (AGPL-compat allowlist), sources (crates.io only), no wildcard versions |
| `secrets` | gitleaks over the **entire history** — a key deleted four commits ago is still published the day the repo opens |
| `sbom` | CycloneDX per crate, kept 90 days with the run |

Hardening that is easy to miss: `permissions: contents: read` at the top, every action pinned to
a **commit SHA** (a tag is a pointer somebody can move), concurrency cancellation, and the
`unsafe_code` forbid in every crate that can carry it — enforced by the compiler, not promised.

**The program keypair never enters CI.** `keys/vitals_program-keypair.json` *is* the program's
identity on every cluster; CI deploys to a throwaway id for exactly this reason.

## CD — deliberate, gated, and half missing on purpose

**The demo** (`cd-demo.yml`): manual button → `demo` environment (required reviewer) → keyless
OIDC exchange into `github-deployer@vitals-academy` (workload identity pool conditioned on this
one repository — **no GCP key exists to leak**) → preflight that the mounted secrets exist →
build via the same `deploy-cloudrun.sh` the human path uses (`PHASE=build|deploy`, so CI and
hand-deploys cannot drift) → **trivy scan between build and deploy**, HIGH/CRITICAL fails →
deploy → smoke-check that the service answers and is connected to a chain.

**The Solana program: no CD, and that is the design.** Deploying it spends the upgrade
authority's signature. Until that authority is a multisig, it is a decision a person takes with
`scripts/deploy-devnet.sh`, which demands `UPGRADE_AUTHORITY` explicitly and verifies the
deployed bytecode as part of the run.

## Local — `scripts/gates.sh`

One command, everything CI checks that can run offline, with graceful skips for tools the
sandboxed machine cannot install (gitleaks, cargo-deny — CI runs them regardless). Chain gates
join in when a local validator is up.

## Secrets policy

| secret | where | why there |
|---|---|---|
| program keypair | this machine + offline backup | is the program's identity; CI uses throwaway ids |
| relay key (demo) | GCP Secret Manager, mounted as a file | the code reads a path; creating it is a decision the preflight enforces |
| ai-review credentials | **none** | Gemini on Vertex through the same OIDC federation — a dedicated SA holding only `roles/aiplatform.user` |
| GCP deploy credentials | **none** | OIDC federation; nothing stored |

## First-run honesty

This workflow set has **never executed on GitHub** — the repo has 27+ unpushed commits including
CI itself. Expect the first run to surface runner-environment issues (solana install timing, a
licence string missing from the allowlist) and budget one iteration for it. `cargo-deny` and
`gitleaks` could not be executed locally (crates.io is blocked from this machine), so their
configs are reviewed but not yet machine-checked.

## Still open, by name

- Branch protection requiring CI on `main` — worth turning on the day the repo opens or a second
  committer appears; today it would only fight the single-operator flow.
- `solana-verify` reproducible builds — blocked on crates.io locally; the day CI can run it, the
  bytecode check upgrades from "matches this machine's build" to "matches any machine's".
- ~~The review API key~~ — gone as a requirement: ai-review runs keyless on Vertex, verified
  answering from both `global` and `asia-southeast1` before this was committed.
