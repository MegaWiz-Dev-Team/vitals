# vitals — Risks

Ordered by what actually kills the project, not by likelihood.

## 1. Timing — resolved 2026-08-23

The countdown is the deadline to **start the timer**, not to submit. Read off colosseum.com and
its Eternal FAQ, which is client-rendered and therefore has to be read with a real browser — which
is why the first attempt could not settle it.

    Eternal window closes    2026-09-07 ~07:00       ← last moment to start a sprint
    start today              submit by 2026-09-20
    start on the last day    submit by 2026-10-05

Eternal is not the hackathon. It is a self-initiated 4-week sprint between the two hackathons
Colosseum runs each year: sign up, click the stopwatch, post a one-minute video update every week,
submit at the end. It is judged on *"ability to prioritize, iterate, and ship"*, so the four weeks
are themselves the exhibit — starting the clock with the work already finished leaves nothing to
show in the weekly updates.

The prize is not the one on the front page. The Eternal Award is **$25,000 non-dilutive USDC**
every six months; the $250,000 is investment from the accelerator's fund, which is a separate
outcome. Teams that have raised venture capital for the submitted product are not eligible.

Colosseum's own FAQ says they *"highly encourage all builders to participate in [the two annual
hackathons] to increase their odds of being selected for the accelerator program."*

**Decision taken 2026-08-23:** skip this Eternal window, target the next full hackathon. Four
weeks is not enough for a product whose market has not been validated, and the odds are better in
the main event by the organiser's own account. The next hackathon's dates are not published on the
site — that still needs asking.

Resubmitting an earlier entry is allowed *"if it has materially changed — a clear pivot,
substantial progress made over several months, and/or meaningful traction."* A pivot must link the
prior submission and explain what changed.

## 2. Patent novelty — the constraint is real, the conflict was not

An earlier version of this file recorded that Colosseum submissions are public and concluded that
submitting and filing a patent were mutually exclusive. **That was wrong, and it was repeated for
a day before anyone checked.** The FAQ says plainly:

> the product GitHub repo — *if closed source you'll need to grant Colosseum access privately*

A private repository with judges granted access is explicitly supported. Submitting does not
require publishing.

**What did happen:** the repository was public from 2026-08-22 to 2026-08-23 — about one day, with
zero forks and zero stars — and was made private again once this was understood. Going private
stops further spread; it does not unpublish what was published. For novelty purposes the test is
whether it was *available* to the public, not how many people took it.

That matters differently by jurisdiction. The United States allows a 12-month grace period for an
inventor's own disclosure, which would put a US filing deadline around 2026-08 the following year.
Europe, China, Japan and Korea apply absolute novelty with no grace period, so rights there may
already be affected. One day and no forks is the best version of this situation to take to a
patent attorney, and it is a conversation to have quickly rather than eventually.

**Resolved 2026-08-24 — by decision, not by legal advice.** The patent is not the moat: the case
library, the faculties already using the product, the conformance suite and the accumulating
outcome data are. The repository opens and a public demo is wanted. What is written above about
jurisdictions remains factually true and is kept for the record — it is the price that was
knowingly paid, not a warning that was missed. Anything still patent-intended in the Embla engine
is a separate question and is unaffected by this repository being public.

**Action:** none for this repository. If a filing is ever wanted for something in the reused
engine, it has to be assessed on its own and the one-day-public fact goes to the attorney then.

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
- **Naming.** `vitals` extends an existing component rather than claiming a new Norse name.

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

