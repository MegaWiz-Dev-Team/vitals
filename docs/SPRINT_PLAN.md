# vitals — 4-Week Sprint Plan

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

## Week 3 — progression (the demo week)

The progression layer is the best three minutes of video in the project: permissionless, recomputed
onchain, and visually obvious. It gets the whole week.

- [ ] `vitals-progress` — integer twin of `xp_for` / `level_for` / `dreyfus`, `no_std`, shared crate
- [ ] Shared test vectors proving the twin matches `competency.rs` on **every threshold boundary**
- [ ] `claim_progress` instruction: merkle proofs in → program recomputes → mint/advance or fail
- [ ] Soulbound mints: Token-2022 NonTransferable — skill-tree per specialty, profile, badges (cNFT)
- [ ] `required_badge` gate on `CaseAccount`
- [ ] Scholarship escrow: sponsor funds a bounty, released on a provably attained badge
- [ ] USDC royalty split on reveal; prepaid pool for institutional seats

Exit: a student levels up, the chain recomputes and agrees, a soulbound token advances, and an
escrowed bounty pays out with no human in the loop.

## Week 4 — credential stub, polish, submission

The SAS competency credential drops to a **stub** — schema registered, issuance demoed for one
domain, thresholds untuned. An institutional credential needs an institution on stage to mean
anything; progression does not. Cut recorded deliberately, stated in the submission.

- [ ] SAS schema + issuance path for one domain
- [ ] Mainnet-beta deploy
- [ ] `proof-check` public verify page
- [ ] Demo video (see below)
- [ ] Open-source release: AGPL-3.0 program + SDK + published case schema
- [ ] Submission write-up + repo hygiene — judges read the repo, not just the video
- [ ] Onboard **3 real case authors** — start Week 1, land by now

## Weekly video updates (required by Eternal)

Do not narrate the roadmap. Show the thing that started working that week:
W1 a case appearing onchain · W2 a score becoming an anchored attestation · W3 **the program
refusing a level-up claim, then accepting the honest one** · W4 the full loop plus a stranger
verifying it on a machine that has never touched our database.

W3 is the money shot. Show the failed claim first.

## Explicitly out of scope

ZK selective disclosure ("top decile without revealing the score") · verifier-node DePIN with
staking · any fungible token · mobile app · new clinical content · **tradeable badges, ever**.

The first five are v2 lines and a judge respects a stated cut more than a half-built feature.
The last is not a scope cut, it is a design decision — see [GAMIFICATION.md](GAMIFICATION.md) §2.
