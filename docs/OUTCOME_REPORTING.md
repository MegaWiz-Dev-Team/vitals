# Outcome Reporting — paying learners for the number that proves the mission


> **Status: scoped 2026-08-24, not built.** Labels: **decided** (founder's call), **measured**
> (checked against code), **open** (unresolved). For the tech session picking this up, and for
> finance/legal who own two of the constraints.

---

## 1. Why this exists

The mission (the internal finance handoff) needs one number:

> **learners scoring ≥ X passed their real examination at Y%**

There are two ways to get it. The institutional route — asking faculties for exam outcomes —
needs IRB approval per institution, a new PDPA basis for record linkage, and **does not exist at
all outside Thailand**, where Vitals is actually going (Indonesia, then Japan, Korea, China).

This route asks the learner directly and pays them for the answer. First-party consent, scales
with users rather than with partnerships, and works in a market where we know nobody.

---

## 2. The rule the whole thing rests on

**Pay the same amount for the report regardless of the result. A learner who failed is paid
exactly what a learner who passed is paid. — decided**

If the payout favours a pass, learners who failed stop reporting, the denominator collapses, and
the measured pass rate converges on 100%, which means the number is worthless and the money was
spent for nothing. This is a validity condition, not a preference.

**On Solana this becomes enforceable rather than promised.** Write the payout instruction so it
never reads the outcome field. Anyone can then verify from the program that we did not quietly
pay more for good news.

---

## 3. Fixing the denominator before the result exists

Equal pay still leaves differential response — people who passed are likelier to come back. So
the cohort must be sealed before anyone knows the outcome:

**The learner pre-registers the sitting** — "I sit examination X on date Y" — as an on-chain
commit, before the result exists. We then chase that fixed cohort and can report a response rate
honestly.

This is **commit–reveal reused**. The primitive that stops a learner practising five times and
anchoring only the good run is the same primitive that stops us reporting only the graduates. Same
shape, second application, and it is already specified for attempts in `docs/ARCHITECTURE.md`.

### What this buys — the strongest claim in the document

With pre-registrations and payouts both on chain, **the integrity of the study is publicly
re-derivable.** The count of learners who committed to sitting an exam is on chain; the count paid
is on chain. Nobody has to trust that we did not discard the failures — they can count.

That converts "trust our pass rate" into "re-derive our pass rate", which is the product's own
thesis applied to our own research. Lead with it.

---

## 4. What exists today — measured

`crates/vitals-program/src/lib.rs` (694 lines) carries `TreeAccount`, `Account`, `ProvenAttempt`,
`ClaimAccount`, `Progress`, with the merkle work in `crates/vitals-progress/`.

**There is no commit account yet.** Commit–reveal is specified in `README.md` and
`docs/ARCHITECTURE.md` but is not in the program. The pre-registration account this scope needs is
structurally the same as the attempt commit already designed — new account, not new architecture.

---

## 5. Verification tiers — payout and claim strength both follow the tier

| Tier | Evidence | Payout | What may be claimed publicly |
|---|---|---|---|
| 0 | self-report, no evidence | **none** | nothing — internal signal only |
| 1 | learner uploads result document | **yes** | "self-reported, document-verified", with n and response rate |
| 2 | institution or board attests via SAS | yes | full strength |

**Build 0–1 now; tier 2 needs institutional counterparties and is a separate piece of work.**
Tier 0 pays nothing precisely because an unevidenced report is the cheapest thing in the world to
fabricate.

---

## 6. Payment design — decided: paid on Solana

**Why the rail is not decoration:** thousands of small cross-border payouts to individuals. Card
rails consume a payment this size entirely. It is also the *same* rail as the case-author royalty
(฿0.5–2 per attempt), so it is one payout system serving two purposes, not a second system.

- **USDC, not a bespoke token.** A purpose-minted reward token would be a tradeable instrument
  attached to a credential product — precisely the thing `docs/TOKENOMICS.md` keeps separate.
  Payout is a payment; it is not progression and must never be confused with it.
- **Gasless.** Fee-payer relayer, as already designed — the learner never holds SOL and never
  sees a seed phrase.
- **The program does not read the outcome.** §2.

### The amount is a finance decision with a real tension — open

Too small and the response rate collapses, which kills the denominator. Too large and fabricated
sittings become worth attempting, which kills the data. **Both failure modes destroy the number,
so the amount cannot be set by intuition** — it needs modelling against expected cohort size, and
it is a genuine COGS line stacked on top of the inference cost rebuild in
the internal finance handoff.

### Anti-farming — the risk changed shape, it did not go away

Paying flat removes the incentive to lie about the *result*. It introduces an incentive to invent
the *person*. Defences, all of which already exist in design:

- pre-registration commit dated before the examination
- tier 1 document evidence gating any payout at all
- the distinct-case, difficulty and variance guardrails in `docs/GAMIFICATION.md`, which were
  written as anti-cheese rules and double as anti-farming rules once real money is attached

---

## 7. Explicitly out of scope

- Different payouts for pass and fail — §2
- Any payout at tier 0
- Long-term storage of the result document itself; store the verification outcome, not the artefact
- Tier 2 attestation in the first phase
- A bespoke reward token

---

## 8. What finance and legal own — open

- **Payout amount and total budget.** §6.
- **Cross-border crypto payments to individuals** in Japan, Korea and Indonesia — rules differ,
  and the payout may be reportable income to the recipient. This belongs in the *same* conversation
  with counsel as the patent gate (the internal finance handoff), not a separate one.
- **PDPA.** New purpose; needs its own consent toggle, not a widening of the existing embla-cloud
  consent. First-party consent is a far cleaner basis than institutional record linkage, but
  publishing results has its own requirements.

---

## 9. Two channels that calibrate each other

Thailand can collect **both** ways — through the 18 faculties already using Embla, and directly
from learners. Everywhere else, only the direct channel exists.

So run both in Thailand deliberately: **if the institutional number and the self-reported number
agree there, that tells us how much to trust self-report in markets where no institutional route
exists.** The Thai study is not just a data source; it is the calibration for the global channel.

---

## 10. Design note for whoever builds the schema

Embla's rubric is **40 points deterministic + 60 points LLM-judged** (`README.md`). Store the two
separately from the first row of data.

If the pass prediction holds on the **40 deterministic points alone**, that is the strongest
version of the result, because a third party can re-derive that score from the transcript without
trusting our model, our version, or us. It would make the outcome study a proof of the product's
central claim rather than a separate marketing exercise.

---

## 11. Deliverable

> **"Learners scoring ≥ X passed at Y% (n = …, response rate = Z%, verification tier 1,
> pre-registrations verifiable on chain)."**

Carry the limit with it: this measures learners in the markets where it was collected. Thai data
does not transfer to Jakarta or Tokyo by assertion — that is inference, not measurement, and it
gets labelled as such.

---

## 13. Proof of licensure — three roles, and one thing we deliberately do not build

**Decided 2026-08-24.** The instinct to use a real licence as proof is right; the object was wrong.
**We anchor the outcome, never the certificate.**

### Ruled out — recorded so it is not revisited

| Proposed | Why not |
|---|---|
| Mint a learner's medical licence as an NFT | `README.md` already argues against exactly this: *"an NFT minted by the same app that produced the score proves nothing."* Licences are already verifiable in authoritative public registers (แพทยสภา, FSMB, GMC) — our copy carries **less** authority than the original, and would look like it carries more |
| We perform primary-source verification with medical schools | This is ECFMG's job, done through EPIC, built on decades of institutional relationships, per country and per language. Not a side feature |
| Any artefact that could function as a licence | An uploaded document turning into an official-looking token is a forgery pathway. We do not build it regardless of intent |

The boundary in one line: **verification pointing inward — granting roles, supplying labels — yes.
Issuing a credential outward — no.** This is the same boundary as *"we can build the protocol; we
cannot grant it standing."*

### Role 1 — evidence for the outcome report

Already the design in §5. The learner pre-registers the sitting, sits it, and presents the result
document. What is anchored is **pass or fail, bound to that learner's anchored practice history** —
not the document. Consistent with §7: store the verification outcome, never the artefact.

### Role 2 — the anti-Sybil key, which closes a gap left open in §6

An official candidate or licence number is the one identifier that is unique **per human in the
examination register**, however many wallets they open.

`Candidate ["cand", exam_id, cand_hash]` — a second payout against the same `cand_hash` fails at
the address level.

**⚠️ It must not be a bare hash.** Licence and candidate numbers are short, structured and often
sequential, so a plain SHA-256 of one can be reversed by enumerating the space in seconds — which
would put a recoverable personal identifier on a public chain and break the "hashes only" boundary
outright.

Use a **keyed hash (HMAC) with a key held by the verifier**, or a deliberately slow KDF with a
per-exam salt. The verifier can still detect a duplicate; a stranger with the chain cannot recover
whose number it was.

### Role 3 — verified licensure unlocks roles inside the system

A learner confirmed to have qualified becomes, in one step, the three things the project most
needs:

- an **outcome label** — §5, the scarcest input we have
- eligible to **author cases** — the paid work that closes the train → qualify → real work loop,
  without ever routing anyone to real patients
- a candidate for the **alumni giving base** that `SYSTEM_DESIGN.md` §10 identifies as the answer
  to lumpy donation income

These are grants of standing *within our own system*, which is the only standing we are entitled to
grant.

### Where SAS belongs — and the direction is the opposite of the proposal

Solana Attestation Service is the right rail for licensure attestation, but **we are not the
issuer.** If a council or faculty chooses to attest, they hold the key and they sign; we provide
the rails and the verification surface.

That is what makes the credential worth anything: **someone else signed it.** An attestation we
signed about our own learners is worth precisely what our own opinion is worth — which is the thing
the entire architecture exists to stop relying on.

**Build order:** Role 2 first (it unblocks the payout design in §6), then Role 1 as part of the
outcome loop, then Role 3. SAS issuance is a later phase requiring institutional counterparties.
