# Security

Vitals anchors clinical-emergency OSCE attempts to Solana. The part that holds value —
the on-chain program — is deliberately small, and that shapes how we read every advisory.

## What runs on chain vs. what surrounds it

The deployed program (`vitals-program`, compiled to a BPF `.so`) depends on
**`solana-program` + `borsh` + this workspace, and nothing else**. It never links the RPC
client, a TLS stack, keypair-signing code, or the test tooling.

Every advisory GitHub/Dependabot flags on this repo lives in that surrounding tooling — the
RPC-over-TLS client, the CLI, proc-macros, test helpers — **not in the code that executes
on chain and custodies records**. An advisory in the client that submits a transaction cannot
change what the program does with it; the program re-derives and re-checks everything itself
(see `docs/INVARIANTS.md`).

## Itemized assessment

The per-advisory triage is version-controlled in **[`deny.toml`](deny.toml)** under
`[advisories].ignore` — each id carries a one-line reason (which tree it enters through, why
it is not reachable from the program). Highlights:

- **RUSTSEC-2026-0104** (rustls-webpki CRL panic, the one "high"): in the RPC client's TLS
  path. Not reachable from the `.so`. Clears when the Solana SDK bumps `rustls` upstream; we
  do not vendor the SDK to force it early.
- **RUSTSEC-2026-0098 / 0099** (rustls-webpki name-constraint / wildcard): same TLS client path.
- Signing, `bincode`, `curve25519`, proc-macro, and unmaintained-crate advisories: all enter
  through the Solana client/test tree; the program tree already sits on fixed versions where it
  matters.

This is not blanket suppression. The list is **exact ids**: a *new* advisory still fails the
build (`cargo deny check advisories` runs in CI), and every entry is owed a re-check at each
SDK bump.

## Reporting a vulnerability

Please report privately via this repository's **Security → Report a vulnerability** (GitHub
private advisories) rather than a public issue. We aim to acknowledge within a few days.
