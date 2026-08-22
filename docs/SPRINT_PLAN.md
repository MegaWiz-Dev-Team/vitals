# embla-proof — 4-Week Sprint Plan

Target: a Colosseum Eternal submission (4-week timed sprint, weekly 1-minute video update),
or the next scheduled hackathon if the Eternal window closes first — see RISKS §1.

Rule for the whole sprint: **anything not demoable on the last day does not get built.**
The head start is real but it is a head start on the *engine*, not on the chain layer.

## Week 1 — the chain skeleton

- [ ] Anchor program scaffold; `CaseAccount` publish/update/deprecate on devnet
- [ ] Commit instruction + PDA, rent-reclaim on reveal
- [ ] `proof-sdk` skeleton: wallet connect, read registry, gasless relayer (fee payer)
- [ ] Wire Embla client → commit before encounter starts (behind a feature flag)
- [ ] **Decide the determinism boundary** (ARCHITECTURE §5) and write it down

Exit: a case is published onchain, an attempt commits, nothing is scored yet.

## Week 2 — the verifier

- [ ] `proof-verifier` service wrapping `embla-engine`; holds issuer key
- [ ] Pin `engine_ver` + `model_id` into the result; extend `audit.rs` events with the anchor
- [ ] Bubblegum tree setup; reveal instruction writes the compressed leaf
- [ ] Re-score path: given transcript + version, reproduce result → byte-identical check
- [ ] Cost measurement on devnet — **publish the real numbers**, do not quote the blog post

Exit: run an encounter end-to-end, the score lands as a compressed attestation.

## Week 3 — credential + money

- [ ] SAS credential + schema registration; issuer authority for one pilot school
- [ ] Threshold rule: N attempts ≥ score in a domain → issue competency attestation
- [ ] `proof-check` public verify page: paste a transcript, get verified / mismatch / not-found
- [ ] USDC royalty split on reveal; prepaid pool for institutional seats
- [ ] Onboard **3 real case authors** — this is the credibility differentiator, start Week 1

Exit: a student finishes a domain and a portable credential appears in their wallet.

## Week 4 — make it land

- [ ] Mainnet-beta deploy
- [ ] Demo video: student runs a case → credential issued → third party verifies it on a
      different machine with no access to our database. That last clause is the whole pitch.
- [ ] Open-source release: AGPL-3.0 program + SDK, published case schema
- [ ] Submission write-up, GitHub hygiene (judges read the repo, not just the video)
- [ ] Business section: existing Embla model + protocol take rate

## Weekly video updates (required by Eternal)

Do not narrate the roadmap. Show the thing that started working that week:
W1 case appearing onchain · W2 a score becoming an attestation · W3 a stranger verifying
a credential · W4 the full loop.

## Explicitly out of scope

ZK selective disclosure · verifier-node DePIN with staking · token · mobile app ·
any new clinical content. Each is a good v2 line; none survives a four-week sprint,
and a judge respects a stated cut more than a half-built feature.
