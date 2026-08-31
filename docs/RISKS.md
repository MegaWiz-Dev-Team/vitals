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

The credential carries both numbers **and their provenance labels**. A badge predicate that would
sit behind sponsor money should be expressible over `det_score` alone — a rule written for the day
that money exists. The program holds no money today; that instruction is not written yet.

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
- **Datasets stay internal.** The Embla corpora do not ship in a public repo. Open-sourcing the
  program and SDK does not mean open-sourcing the case library or the KB.
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

## 9. Sponsor money would turn badges into a farming target

Scholarship bounties (GAMIFICATION §3 — *coming next*, not built) would give badges cash value,
which invites grinding, shared
accounts, and script-driven attempts. The engine's existing gates help — distinct-case requirements,
difficulty weighting, the variance cap, exam-mode weighting — and commit–reveal makes the attempt
denominator public, so a farmed badge carries a visible attempt count.

That makes farming **legible**, not impossible. A predicate with sponsor money behind it would
need first-attempts-only and ranked-mode flags on top. Do not ship a bounty against a
practice-mode predicate.

## 10. The pitch can become a grab-bag

Credentials, attempt anchors, progression NFTs, badge-gated cases, author royalties, scholarship
bounties — six things. A judge who cannot restate the project in one sentence scores it as unfocused
regardless of how well each piece works.

**The sentence:** *one anchored record, read at three resolutions — attempt, progression, credential.*
Everything else is a consequence of that. If a feature cannot be introduced as a consequence of that
sentence, cut it from the pitch even if it is built.


## 11. Silence in the reply marked a harmful order — measured, disclosed, closed 2026-08-31

**Closed, by re-issuing all twelve stations so that no order ever replies with nothing.** The
numbers first, measured with the same method before and after — all twelve stations, every
reachable order, one fresh run each, counting narrative beats in a sealed reply:

    before (2026-08-29)   P(no beat | harmful)   0.82    18/22
                          P(no beat | harmless)  0.06    11/176     LR = 13.1
    after  (2026-08-31)   P(no beat | harmful)   0.00    0/22
                          P(no beat | harmless)  0.00    0/176      LR = 1

A silent reply no longer exists, on either side, so it can no longer carry information about
anything. The likelihood ratio is 1 by construction rather than by luck: the event being
conditioned on cannot occur.

**What it was.** The harm marker itself was sealed in Week 1 — no harm row on the chart, no harm
line in the feed, nothing in the result panel until the bell. What was not sealed was the
absence: the engine emits exactly the beats a case declares, and the authors had written
narrative lines for the right answers and none for the traps. In the files as shipped, 19 of 19
harmful interventions carried no `beat` effect against 13 of 179 harmless ones. So ordering the
trap returned a reply one line shorter than ordering anything else, at a likelihood ratio of
13.1 — quoted as roughly 14× when first disclosed; (18/22)/(11/176) is the arithmetic — invisible
on screen, readable in a network tab, real. The cause was case content,
not engine code, which is why it was disclosed and left open rather than patched in the engine:
the two closures available at the time — editing anchored scenario files, or suppressing the
`threshold:` beats that *are* the examination — both cost more than the leak.

**What closed it.** The twelve stations were re-issued under the retirement flow in
`conformance/README.md`: every trap intervention, every quiet harmless order (the oxygen, the
saline, the positioning), and every trigger that records a harm now carries a narrative beat, in
the same observational register the harmless orders already used — the room noting what was
done, never a verdict. The right answer keeps its editorial tail; the trap reads as
unremarkable procedure; the beat never announces the harm, because announcing it would recreate
the leak this replaced. Every new line has a Thai row in the language layer, through the same
table as every line before it — 282 distinct case-scripted lines across the shelf today, 296
table rows once the engine's own vocabulary is counted, and the number is derived rather than
remembered: `every_scripted_beat_of_every_case_has_a_thai_line` in `crates/vitals-web/src/lang.rs`
recounts the files on every `cargo test` and fails the build if any case gains a line the table
lacks. Language never reaches the leaf.

**What the re-issue did to hashes, and to nobody's proofs.** `sce_hash` is
`sha256(<the whole scenario file>)` and the leaf commits to both the hash and the beats, so all
twelve identities rotated — that is the designed cost of touching a case, and the supersede flow
paid it: the outgoing version of every station was already archived under
`conformance/sce-archive/` with its `INDEX.json` row, the incoming versions are archived beside
them, and `crates/vitals-replay/tests/shock_tape.rs` holds the leaf of every archived version
exactly where it was. The five attempts anchored on devnet against `osce-a` name the archived
bytes and re-derive from them unchanged. A deployment pins an image, so the closed state reaches
devnet with the next deploy, not before.

**What pins it shut.** `crates/vitals-replay/tests/trap_silence.rs`, run on every `cargo test`:
it replays every order of every station in a fresh run and fails if any sealed reply is empty,
and it walks every intervention's effect tree and every trigger off disk and fails on any silent
path — so a week-9 author who writes a trap without a beat is stopped by the build, not by a
reviewer's memory. The residual channel was measured too: reply *counts* still vary (18 of 22
harmful replies carry 1 beat and 4 carry 2, against 171 and 5 of 176 harmless), but every
2-beat reply is an order that changed the patient's status, and the status is printed on the
screen beside it — the count now tracks what the candidate is already shown, and nothing else.

**What is not a leak, and will not be treated as one.** A candidate can tell they did harm by
watching the patient. On `osce-d3` the adult dose in a twenty-kilo child runs her to HR 171 where
the correct dose holds 136; on `osce-a` IV-push adrenaline drops him to 76 systolic. Nothing labels
those numbers, nothing timestamps them against a named order, nothing calls them harm. **Noticing
that the patient got worse after what you did is the skill being examined**, and it is not going to
be hidden. A monitor that stayed flat through a mistake would be the actual integrity failure.

### 11a. The episodes carry the same authoring pattern, weakly — measured, disclosed, open

The file-level half of §11's method, run over the five episodes on 2026-08-31 and independently
re-counted from the files by the video team: harmful interventions with no `beat` effect —
EP3 2/3, EP4 1/1, EP5 1/1, EP2 0 (its one trap is a conditional branch that already narrates).
The harmless side is quiet too — EP2 4/9, EP3 3/5, EP4 3/8, EP5 3/6 — so the separation is far
weaker than the 13.1 the stations carried, over a handful of traps, and it exists only on the
exam path: an episode is sealed only when played as a declared exam, and in practice the harm
line prints where it happens, verdict and all.

It is still the same tell in the same place, and it closes the same way. EP2–EP5's current files
are unanchored — every anchored episode run names an archived version — so their re-issue costs
exactly what the stations' cost, at the next legitimate hash change. EP1 is the exception: its
file is frozen against `conformance/ep1-vectors.json`, whose generator lives outside this
repository, so it re-issues when the vectors regenerate or not at all — and its own numbers
(2 of 2 harmful silent, 9 of 9 harmless silent) separate nothing anyway. `trap_silence.rs` pins
the stations only, deliberately; widening it to `demo/scenarios/` is the acceptance test of the
episode re-issue, and this row is where that work points until it lands.
