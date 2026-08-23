# vitals — Fit against Colosseum's judging criteria

The six criteria are quoted from the Solana Cypherpunk Hackathon Official Rules §8.
Written as an honest self-assessment, including where we are weak.

## 1. Functionality — "How well does this work? What is the quality of the code?"

**Strong.** Most entries demo a prototype built in four weeks. We demo a chain layer built in
four weeks on top of a production-shaped Rust engine with a deterministic rubric scorer, a
hash-chained audit log cross-validated across two implementations, 424 authored scenarios in production across 30 medical institutions (>75% of Thailand), and
real users. The repo will show engineering history, not a hackathon sprint's worth of commits.

## 2. Potential Impact — "TAM? Impact on the broader Solana ecosystem?"

**Strong on the vertical, medium on the ecosystem.**
- Immediate market: ~100k Thai health students, 23+ medical schools (already active in >75% of Thai medical faculties); scaling to 4,350+ global medical schools on Solana.
- The larger claim is credentialing generally — clinical skill is the hardest case (high stakes,
  heavily regulated, fiercely protective of data). Solving it here generalises downward to every
  other skill credential.
- Ecosystem impact: brings a non-crypto professional vertical onchain with real non-speculative
  transaction volume, and puts SAS to work on something other than KYC.
- **Weakness to own:** we are not bringing DeFi liquidity. If the judges want TVL, this is not
  that project. The counter-argument is that millions of annual attestations from a regulated
  vertical is exactly the durable, boring volume the ecosystem says it wants.

## 3. Novelty — "How unique is this concept?"

**Strong, if pitched correctly.** "Credentials on chain" is a 2017 idea and "achievement NFTs" is
older still; judges will pattern-match within ten seconds. The novelty is not storage, it is that
**the program computes the predicate instead of trusting an issuer**:

- Commit–reveal ordering makes retry-farming visible — the chain knows the denominator.
- `det_score` is re-derivable by any third party at the pinned engine version (RISKS §3).
- Progression is **permissionless**: `claim_progress` hands the program merkle proofs, the program
  runs its own integer copy of `level_for`/`dreyfus`, and mints only if its arithmetic agrees.
  Every other achievement NFT is minted because a server said so.

Lead with the failed claim, then the honest one. Never open with the word "certificate", and never
open with the word "badge".

## 4. UX — "How well does this use Solana's performance to create great UX for downstream users?"

**Strong.** The downstream user is a medical student who must never learn what a seed phrase is.
- Gasless via fee-payer relayer; wallet is created silently, recoverable via the school SSO.
- The attestation lands inside the after-action report — seconds after the encounter, in the
  same screen where the score appears. On a slow chain this becomes a "pending" spinner and the
  credential stops feeling like a result.
- Volume economics are a UX property here: at ~$0.0001 per anchor we can anchor *every* practice
  attempt, not just exams. Anchoring only the exams would be the cheap-chain compromise, and it
  would destroy the continuous-competency thesis.
- The progression layer is where a student actually feels the chain: the skill tree advances a
  Dreyfus stage the moment the attempts justify it, in the same screen as the score. Sub-second
  finality is what makes that read as a game rather than as paperwork.

## 5. Open-source + composability — "Is it open-source? How well does it compose with other primitives?"

**Strong.**
- AGPL-3.0 + Commercial (Asgard policy). OSI-approved open source; the program, SDK and case
  schema are public.
- Composes with **Solana Attestation Service** (credential issuance), **Metaplex Bubblegum**
  (compressed attempt anchors), **Token-2022** (royalty splits). We add a case registry, not
  a new standard where one already exists.
- The registry is readable by anyone: a competing trainer can serve the same cases, pay the same
  authors, and issue against the same schema. Designed as a protocol with a reference client.
- `required_badge` puts prerequisite gating **in the registry rather than in our client**, so a
  competing front-end inherits it for free. Composability as a concrete artifact, not a claim.
- All progression tokens are Token-2022 **NonTransferable**. We give up secondary-market volume on
  purpose: a tradeable "Expert in Cardiology" is credential fraud with extra steps.

## 6. Business Plan — "Is there a viable business here?"

**Strong — it already has revenue mechanics.** Embla's model carries over: student subscription,
institutional on-prem seats, and now a protocol take rate on the case marketplace. The onchain
layer adds a business the old model could not have: independent case authors anywhere in the
world earning per attempt, without an acquiring relationship in each country.

Unit economics are inherited, not invented for the pitch: ~80% gross margin on subscriptions,
near-zero COGS on institutional on-prem because inference runs locally.

## The four questions to rehearse

1. *"Why does this need a blockchain?"* — Because the verifier must not be the same party as the
   scorer, and the relying party is in a different country. Take the chain away and you are back
   to trusting one company's database, which is exactly why current digital credentials are worthless.
2. *"How do I know the student took the exam?"* — You don't, from us. We prove the scoring was
   faithful to the transcript; identity binding is the issuer's job and the credential names the
   issuer. Answering this cleanly is worth more than pretending it is solved.
3. *"Isn't this just an NFT certificate / achievement badge?"* — Show two things. The re-derivation:
   two machines, no shared database, same transcript, same `det_score`. Then the refused claim:
   submit a level-up the attempts do not justify and watch the program reject its own user. A
   certificate cannot do either.
4. *"Which part of the score is actually provable?"* — 40 of 100 points, by weight, and we say so
   before being asked (RISKS §3). The other 60 are verifier-quorum attested and labelled as such.
