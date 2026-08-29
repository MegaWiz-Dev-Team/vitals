# vitals — Risks

Ordered by what actually kills the project, not by likelihood.

## 1. Timing — the calendar is external, and it was read wrong once

The dates this project runs against are set outside it. That is not the interesting part. The
interesting part is that this file recorded them as unknown when they had been published all
along, a plan was made against dates that were not the real ones on 2026-08-24, and it was
reversed the same day once the real ones surfaced.

Two things caused that and both are cheap to avoid next time. The pages carrying the dates are
client-rendered, so a fetch returns an empty shell and a real browser is needed to read them —
the first attempt came back with nothing and that was taken for "not published". And once a date
was assumed rather than read, everything downstream of it was planned against the assumption
without anyone going back to check the input.

**The rule this leaves behind:** read the calendar off the source, with a browser that runs the
page, before deciding anything against it — and re-read it before acting on a decision that is
more than a day old.

What survives as a constraint is the shape rather than the dates: a fixed preparation window and
then a fixed, short build window, neither of which this project sets. §8 is the direct consequence
— it is the scope that fits inside that — and every other section in this file is written against
that clock rather than an open-ended one.

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


## 11. Silence in the reply still marks a harmful order — measured, disclosed, not closed yet

The harm marker is sealed during a station. As of 2026-08-29 the chart carries no harm row, the
encounter feed carries no harm line, and the result panel's list is empty until the bell
(`1a9728e`, `39a08bb`). A candidate is no longer told mid-station that an order was wrong.

**What was not sealed is the absence.** Measured across all twelve stations, every reachable
order, one fresh run each:

    P(no beat in the reply | the order was harmful)   0.82    18/22
    P(no beat in the reply | the order was harmless)  0.06    11/176

So the **number of narrative beats in the JSON reply** still separates a harmful order from a
harmless one, at a likelihood ratio of roughly 14×. Order the trap and nothing comes back; order
anything else and a `threshold:` line usually does.

**The cause is case content, not code.** An author writes the right answer a narrative line —
*"adrenaline 0.5 mg im, outer thigh — no hesitation"* — and writes the trap beside it none, because
the trap felt like it had nothing to say. In the files themselves, 19 of the 19 harmful
interventions declared across the twelve stations carry no `beat` effect against 13 of 179
harmless ones — a cleaner split than the runtime figures above, which are over reachable orders in
a live run, where a handful of harms arrive from `triggers` instead and a handful of orders pick up
a beat from a state change. No line of the engine treats a harmful order differently. The engine
emits what the file told it to emit, and the file is quiet in exactly one place.

**What it takes to exploit.** Open a network tab, count the beats in `/api/step`, and infer across
runs — because 0.82 is not 1.0, one silent reply is evidence and not an answer. It is invisible on
screen, it survives no single observation, and it costs a candidate the attention they were meant
to be spending on the patient. It is strictly weaker than what it replaced, which was a visible
certainty delivered unprompted. It is still real, and it is still a channel the exam did not
intend to open.

**What it would cost to close today.** Two routes, both worse than the leak:

- *Give the traps their missing beats.* That means editing the scenario files, and `sce_hash` is
  `sha256(<the whole scenario file>)` — the case's identity on chain. Every proof already anchored
  against those cases would name a file that no longer exists, including the run a judge is asked
  to re-derive from the video. See `conformance/README.md`.
- *Suppress `threshold:` beats in exam mode.* Those lines are the ECG read, the auscultation
  finding, the afebrile child, the nurse asking how many milligrams. They are the examination.
  Deleting them to hide a correlation would delete the thing being examined.

**When it closes.** At authoring time, and it lands when a station is next retired and re-issued —
which is when its hash legitimately changes anyway. The rule is written down where the next author
will meet it (`conformance/README.md`, *Authoring rules that are not in the schema*): **every
intervention gets a narrative beat, including the harmful ones**, and the re-issue checklist audits
for it before a station goes back on the shelf. Nothing about this is waiting on a decision; it is
waiting on the next hash change, deliberately.

**What is not a leak, and will not be treated as one.** A candidate can tell they did harm by
watching the patient. On `osce-d3` the adult dose in a twenty-kilo child runs her to HR 171 where
the correct dose holds 136; on `osce-a` IV-push adrenaline drops him to 76 systolic. Nothing labels
those numbers, nothing timestamps them against a named order, nothing calls them harm. **Noticing
that the patient got worse after what you did is the skill being examined**, and it is not going to
be hidden. A monitor that stayed flat through a mistake would be the actual integrity failure.
