# embla-proof — Risks

Ordered by what actually kills the project, not by likelihood.

## 1. Timing — the Eternal window may close before a 4-week sprint fits

As of 2026-08-22 the Eternal countdown showed **~15 days**. A 4-week sprint does not fit inside
15 days. Two readings: the countdown is the deadline to *enter and start the timer*, or it is the
deadline to *submit*. The site does not disambiguate and the FAQ answers are client-rendered.

**Action before anything else:** confirm with Colosseum which it is. If it is the entry deadline,
the timer must start within ~2 weeks. If it is the submission deadline, target the next scheduled
hackathon instead and use the extra runway. Do not start building on the assumption.

## 2. Public submission destroys patent novelty

Colosseum's rules state entrants should not assume any confidentiality in their submission, and
submissions are public. If any part of the Embla scoring/competency work is patent-track, filing
must happen **before** submission — the same sequencing already established for trial registration.

**Action:** audit what in `embla-proof` and the reused Embla engine is patent-intended. File first,
or consciously accept it as disclosed. This is a one-way door.

## 3. The determinism boundary — worse than assumed, and now measured

Reading `docs/SCORING.md` in Embla settles the open question from the kickoff, badly:

| | weight | scored by |
|---|---:|---|
| diagnostic_accuracy | 25 | deterministic |
| investigation_choice | 15 | deterministic |
| history_completeness | 15 | **LLM judge** |
| management_safety | 15 | **LLM judge** |
| communication | 10 | **LLM judge** |
| examination | 10 | **LLM judge** |
| red_flag_recognition | 10 | **LLM judge** |

**40 points deterministic, 60 points LLM-judged** (`gemma-4-26b` via Heimdall). So the headline
claim "a third party can re-derive the score" is, as written, true of 40% of the score.

Greedy decoding at temperature 0 is not a fix. It is reproducible on the same binary and the same
hardware; it is not reproducible across MLX versions, Metal kernel changes, or batch shape. Pinning
`model_id` narrows the claim, it does not establish it.

**Resolution — split the score, and say which half is which:**

- `det_score` (40) — **re-derivable**. Anyone re-runs `embla-engine` at the pinned version and must
  reproduce it byte-for-byte. This is the mathematically strong claim.
- `judged_score` (60) — **quorum-attested**. One or more verifiers sign "I ran model X at version Y
  over this transcript and got these dimension scores." Multiple independent verifiers raise
  confidence; none of them make it re-derivable.

The credential carries both numbers **and their provenance labels**. Badge predicates that sit
behind escrow money should be expressible over `det_score` alone.

This is a stronger position than the vague version, not a weaker one: knowing exactly which points
are proof and which are attestation is a more sophisticated answer than any competitor will give.
But it must be in the README and in the pitch, not discovered by a judge in the Q&A.

## 4. Identity binding is not solved

We prove the scoring was faithful to a transcript. We do not prove who produced the transcript.
This is a genuine limitation, not a gap to paper over. Mitigation is delegation: the issuer
(school) binds identity via SSO/proctoring, and the credential names the issuer so a relying party
prices the trust accordingly.

**Action:** say it first, in the pitch, before a judge says it for us.

## 5. "Why blockchain?" is the default skepticism, and medicine amplifies it

Healthcare-on-chain has a bad track record and judges know it. Anything that smells like
"health records on the blockchain" gets discounted immediately.

**Mitigation:** be aggressive that **no patient data and no student PII touch the chain** — hashes
and pubkeys only. Virtual patients are synthetic (DDXPlus, CC-BY-4.0), so there is no PHI in the
system to leak in the first place. This is a defensible position, but only if stated up front.

## 6. Constraints inherited from Asgard that the chain layer must not break

- **Inference stays local.** Eir/clinical reasoning runs on Heimdall; cloud LLM is not an option.
  The onchain layer must never require shipping clinical content off the box for a demo's convenience.
- **No student PII onchain** — PDPA. Performance data is personal data.
- **Datasets stay internal.** Embla corpora live on the T7 SSD and do not ship in a public repo.
  Open-sourcing the program and SDK does not mean open-sourcing the case library or the KB.
- **License is AGPL-3.0 + Commercial**, never MIT/Apache. This satisfies the open-source criterion
  without giving the commercial position away.
- **Naming.** `embla-proof` extends an existing component rather than claiming a new Norse name.

## 7. Institutions will not transact in crypto

Thai medical schools buy on invoice, in baht, through a procurement process. Any design that
requires a university to hold USDC is dead on arrival.

**Mitigation:** already in the architecture — schools stay on fiat rails and the platform settles
author royalties onchain on their behalf. Keep it that way even though a pure-onchain story would
pitch better; a pitch that cannot survive contact with a real customer is worth nothing.

## 8. Scope

The engine head start creates a temptation to promise the ZK selective-disclosure version, the
DePIN verifier network, and a token. Four weeks buys the commit–reveal loop, the credential, and
the payment split. Nothing else.

## 9. Escrow money turns badges into a farming target

Scholarship escrow (GAMIFICATION §3) gives badges cash value, which invites grinding, shared
accounts, and script-driven attempts. The engine's existing gates help — distinct-case requirements,
difficulty weighting, the variance cap, exam-mode weighting — and commit–reveal makes the attempt
denominator public, so a farmed badge carries a visible attempt count.

That makes farming **legible**, not impossible. Escrow-backed predicates need first-attempts-only
and ranked-mode flags on top. Do not ship a bounty against a practice-mode predicate.

## 10. The pitch can become a grab-bag

Credentials, attempt anchors, progression NFTs, badge-gated cases, author royalties, scholarship
escrow — six things. A judge who cannot restate the project in one sentence scores it as unfocused
regardless of how well each piece works.

**The sentence:** *one anchored record, read at three resolutions — attempt, progression, credential.*
Everything else is a consequence of that. If a feature cannot be introduced as a consequence of that
sentence, cut it from the pitch even if it is built.

