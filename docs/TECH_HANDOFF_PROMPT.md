# Prompt for the tech session

Rewritten 2026-08-24 after seven decisions were taken. **Supersedes the earlier version — two
things it forbade are now required.** Copy everything below the line.

---

You are picking up **Vitals** (`/Users/mimir/Developer/vitals`) for the **Colosseum fall
hackathon: 28 September – 2 November 2026.** Eternal was considered and deliberately skipped.

That gives two phases, and the split is the point:

```
now → 27 Sep     PREPARATION, five weeks, no external deadline and no weekly video obligation
28 Sep → 2 Nov   HACKATHON, five weeks, what the judges see
```

Read first — these carry decisions already taken; do not re-litigate them:

- `docs/CHECKLIST.md` §A — the seven decisions, and what they retired
- `docs/SYSTEM_DESIGN.md` — the whole system and why each part exists
- `docs/THEORY_OF_CHANGE.md` — what the code has to be true for
- `docs/TECH_PLAN_COLOSSEUM.md` · `docs/CASES_FROM_EMBLA.md` · `docs/OUTCOME_REPORTING.md`
- `docs/PITCH_FRAME.md` — what the pitch claims, so you know what the code must support

**Frame for every call:** does this make us look like a company that can go devnet → mainnet in
eight weeks?

**The advantage of this plan, and it is the reason it was chosen:** the repository opens and the
public demo can go live *during preparation*. So by 28 September we can arrive at the main event
with **real users and real usage**, not only a working product. Nothing about that is possible if
the build starts when the hackathon does — treat the five preparation weeks as the thing that
makes the submission different from everyone else's.

## What changed — two reversals, read them before planning

1. **The repository opens, and a public demo is now wanted.** The patent gate was closed by
   decision: the patent is not the moat — 433 cases, 18 faculties, the conformance suite and the
   accumulating outcome data are. Earlier documents forbidding a public URL are superseded. B2C
   acquisition depends on that URL existing.
2. **The donation Commission is demo-critical, not "designed, not built."** Story-mode content is
   frozen at five scenarios; instead, a **visible funding goal for scenario six** is what
   demonstrates the donation loop. "We only have five" becomes "here is how the sixth gets made,
   and you can audit the money."

## What goes in which phase

**Preparation — now to 27 September.** Foundations, plus everything that needs *time to
accumulate* rather than time to build.

```
Commit instruction + account · AnchorReplay requires and consumes an open commitment ·
wire into run start · SHOW the start-count in the UI
  → the headline integrity claim, currently in writing with no code behind it

vitals-osce — the deterministic 40 only (score_sop + score_deterministic and its grading
helpers), dependency-free, mirroring vitals-sce · its own conformance vectors

Practice-mode encounter surface in vitals-web · unlock predicate computed by the program
from Progress (distinct_cases / level)

Cases: register vitals as a deployment target · widen content_hash · ProvenAttempt.case
Store the deterministic 40 and LLM-judged 60 as separate fields — see below

Publishing review, then OPEN THE REPO and put the public demo up with rate limits
  → this is the item that must not slip to the hackathon phase. Users take weeks to arrive;
    code does not. Ship it early and let it accumulate
  → but NOT before commit–reveal. See the ordering note below: this block is not a list of
    parallel tasks

In parallel, not blocked by any of the above:
  · run psychometrics.rs over the 671 real runs — SEM gates every later prediction claim
  · willingness-to-pay test on the 290 existing users
  → both produce numbers the submission can state, and neither needs the chain
```

**Hackathon — 28 September to 2 November.** What judges see.

```
Commission account + treasury · public donor page (raised / goal / what shipped)
Deck and video (Skald · vitals-pitch/episodes already has a storyboard and stills)
Polish · submission
```

Add to this phase, because it is real work that neither phase currently owns: **running the
service.** Opening the demo in preparation means it is live for ten weeks, not five, and the second
five are spent building the Commission and shooting video while strangers use it. Three things are
not ready for that audience today:

- **The model ceiling is global.** `SAY_PER_MIN = 20` counts across all callers, not per caller —
  twenty legitimate people in a minute and the twenty-first gets nothing, and one script can hold
  the whole demo down for free. `embla-cloud/src/ratelimit.rs` exists and can be borrowed.
- **The page's bearer token is CSRF protection, not access control.** It is embedded in a page
  anyone can load, so it stops another site acting on a visitor's behalf and stops nothing else.
  That distinction was academic while the audience was us.
- **The upgrade authority is one hot key**, and `INVARIANTS.md` says so in a section that becomes
  public with the repository: *every invariant above describes bytecode one key can replace*. That
  sentence is honest and belongs there. Better to make it obsolete before judges read it — moving
  to a multisig costs nothing and does not wait on mainnet.

### The one ordering that is not negotiable

**Commit–reveal ships before the demo opens.** The preparation block reads as a set of parallel
tasks and these two are not.

Runs anchored before commit–reveal exists carry no commitment. If the demo opens first, the weeks
of real usage this plan exists to accumulate become usage of the product *without* its novelty —
and the answer to "show me a real user's committed run" is "those start in week five", which is the
opposite of the advantage being bought. Whether that is merely awkward or actually a migration
depends on a question nobody has answered yet:

> **Does the anchored record carry evidence that a commitment existed?**
>
> If it does, `AttemptRecord` grows a field and every leaf changes — free while nothing is anchored
> anywhere, a migration of real users' records afterwards. If it does not, the rule is enforced at
> anchor time and cannot be shown from the record later, which weakens exactly the shot the video
> ends on. Answer it before writing the instruction, not after.

The tape format and the tree PDA were both changed on 24 August for precisely this reason, while
the cost of changing them was zero. That window closes the day the demo opens.

**The shot that ends the video:** commit–reveal on an **OSCE station**, and the unlock the chain
checked.

*Not on story mode — "you cannot practise the station five times and anchor only the good run" is a
sentence about an examination.*

## Ground truth in the code

`Instruction` today: `OpenAccount | AddAuthority | RemoveAuthority | AnchorReplay | ProveAttempt |
ClaimProgress`. **There is no commit** — but `README.md` and `ARCHITECTURE.md` present
commit–reveal as the novelty, and judges have repo access. Build it or stop claiming it.

`ProvenAttempt` already carries `exam_mode: bool`, so **practice mode needs no program change**.
`chain.rs` already does `prepare_anchor → submit → anchored → prepare_claim → claimed` with a
fee-payer relayer, so the learner never holds SOL.

Embla's deterministic/LLM split is already clean: `engine/src/examiner.rs` says so in its own
module doc, and `score_deterministic()` sits beside `score_sop()`. All pure functions over a case
and an event list — no DB, no HTTP, no LLM.

## Inference — decided: hybrid, and the exception is deliberate

Cloud for the public demo; Asgard/Heimdall nodes for base load as they come online. The standing
rule is *Heimdall only, no cloud LLM for Eir agents*; **this is a recorded exception, not an
oversight** — synthetic-patient dialogue carries no PHI and is not clinical care, the same shape as
the CloudyFarm exception to Rust-first. Write down that it is an exception where the code lives.

**Rate limits are part of the product, not an afterthought.** Free to everyone plus paid inference
means a stranger can spend our money. Per-user limits, a monthly ceiling, and when the ceiling is
reached the page says what this month's compute funded and shows the donation counter. **The
constraint is visible on purpose** — it matches the on-chain transparency of the treasury.

## Cases

Nothing migrates. `embla-cases` stays the single source of truth, consumed as a pinned dependency
as it already is by Embla and Askr. Register `vitals` as a third target via `deploy-track.py
record`. Then:

- **Widen `content_hash` to the full SHA-256** (currently truncated to 16 hex / 8 bytes; the
  on-chain field is 32). Keep the short form as a label. Add `rubric_hash`
- **`ProvenAttempt.case` = the full content hash**, so an anchored attempt names the exact case
  content a verifier must score
- Refuse to run on a case whose hash does not match `deployments.jsonl` — drift fails loudly

## Four traps — each looks obviously right and would break something

1. **The `hidden` block must never be published — and this one survives the repo going public.**
   It is Askr's exam security, not patent strategy. The learner's client gets the stripped bundle
   from `deploy-strip.py`; **the scorer runs server-side with the full case.** Open algorithm,
   controlled content. `vitals-sce` compiling to wasm is so auditors can re-derive — not so the
   client can score itself.
2. **Do not backfill the 671 historical runs into the anchored tree.** A historical attempt cannot
   be retroactively committed, so it would sit there looking identical to committed records while
   being unable to make the claim the tree exists for. Import Embla standing as an explicitly
   labelled *attested* starting position instead.
3. **OSCE conformance vectors would publish answer keys.** SCE vectors are safe; an OSCE vector
   needs a case *and* its expected rubric output. Author **one public specimen case**, marked
   never-deployable, and use only that.
4. **`cand_hash` must not be a bare hash.** Licence and candidate numbers are short and often
   sequential — a plain SHA-256 is reversible by enumeration, which would put a recoverable
   personal identifier on a public chain. Use HMAC with a verifier-held key.

## Nearly free now, expensive to retrofit

**Store the deterministic 40 and the LLM-judged 60 as separate fields from the first row of data.**
If the pass-rate correlation later holds on the 40 alone, that is the strongest possible version of
the result — a third party can recompute it without trusting our model, our version, or us. It
cannot be recovered if the two were ever summed.

## Not in this sprint

- Outcome-reporting instructions (`OpenSitting` / `SubmitOutcome` / `ClaimReward`) — specified in
  `OUTCOME_REPORTING.md`, blocked on legal. Present as designed, not shipped
- SAS attestation — needs institutional counterparties
- More story-mode scenarios — frozen at five by decision
- Horizontal scale for the anchoring service. Have the answer ready instead: `tree_seeds` makes a
  tree per cohort or season by design, so it is a sharding question, not a rewrite

## What is actually blocked

The hackathon dates were published all along (spring 6 Apr–11 May, fall 28 Sep–2 Nov); earlier
drafts recorded them as unknown, which was wrong. Eternal's weekly-video question is moot now that
Eternal is skipped.

But "nothing is waiting on an answer" is not true, and the three things that are waiting all block
**the same item the plan calls unslippable** — opening the demo. Checked against the machine on
24 August, not inferred:

| Blocked on | State | Who unblocks it |
|---|---|---|
| A product name | Not decided | Founder. It names the GCP project, which names everything else |
| A GCP project | Cannot be created without the name | `deploy-cloudrun.sh` refuses to run without one, and refuses `cloud-super-hero*` outright — that project holds Embla's identified users under a recorded consent version. The guard is correct; it is also now on the critical path |
| SOL on a public cluster | **0 SOL on devnet, and the program has never been deployed there** — the account does not exist | The faucet has refused every attempt. Needs ~1 SOL for the program account alone (0.78 rent-exempt minimum at 121 KB), plus fees and per-player rent. Call it 2 |

None is hard. All three take days of calendar rather than hours of work, which is exactly the kind
of thing that is discovered in week four of five.

**Also not a technical dependency, and not free either:** the psychometrics run reads 671 real
scored runs that live in `embla-cloud`'s production Firestore, attached to identified users under
`consent_version = "2026-06-13"`. Aggregate statistics over them is a benign secondary use and
almost certainly fine — but it is a decision someone makes on purpose, with outputs kept aggregate,
not a `cargo run`.

## The risk to watch in yourself

The Commission and outcome reporting are new and interesting; commit–reveal is finishing something
already promised in writing. The failure mode is building the exciting one and arriving at
28 September with the headline claim still unimplemented.

**Second failure mode, specific to this plan:** treating the preparation weeks as slack because
there is no external deadline. The five weeks are the entire advantage — spend them and the
submission is ordinary.

And: **a failing conformance test is never fixed by regenerating the vectors.** If the two
implementations disagree, one is wrong, and the vectors are how you find out which.
