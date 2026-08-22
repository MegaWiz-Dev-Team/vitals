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

## 3. The determinism boundary

The pitch rests on "a third party can re-derive the score." That is true of the rubric scorer and
false of anything LLM-graded. If an LLM-assisted dimension is inside the anchored score, a judge
who probes will find the claim is only partly true, and the whole credibility of the pitch goes
with it.

**Action:** split the score. Anchor the deterministic part as re-derivable; anchor any LLM-graded
part separately and label it advisory. Decide in Week 1, state it in the README.

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
