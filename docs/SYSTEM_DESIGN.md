# The whole system


> Written 2026-08-24 at the founder's request: design it properly, with revenue set aside.
> Labels: **built** · **specified** (in our docs, not in code) · **new here**.
>
> Mission: *increase the number of newly graduated doctors worldwide by 1% per year.*
> Nothing below is here for any other reason.

---

## 1. The one idea

Every credential in the world today is **asserted**. A board says you are competent; you carry a
document that says so; a relying party either trusts the issuer or does not. The threshold — what
separates "competent" from "not" — is set by committee and defended by authority.

The system described here sets its thresholds **from outcomes, and then lets anyone check the
arithmetic.**

> A learner practises. Every attempt is anchored. Some of those learners later sit a real
> licensing examination and report what happened. The correlation between anchored score and real
> outcome is computed in the open — and **that correlation is what defines where the badge
> threshold sits.**

"Proficient in Emergency Medicine" stops being a name someone chose. It becomes *the score at
which observed pass probability crosses a stated line*, recomputable by anyone from public data.

**This is the join that was missing.** Until now the outcome study was framed as proof that the
product works — a marketing asset. It is more than that: **it is the calibration input for the
credential itself.** Without it the thresholds are arbitrary. With it, the credential is the only
one in medical education that can state its own predictive meaning and show its work.

Everything else in this document exists to make that sentence true and keep it true.

---

## 2. The learner loop

One loop, not two modes.

```
   a stranger arrives
        │
   [1]  one scenario, free, no account, no medical vocabulary needed
        │   a patient is dying and you decide what happens next
        ▼
   [2]  they want more  ──────────────────────────────┐
        │                                             │
   [3]  OSCE stations — the real thing                │
        │   history, examination, diagnosis, management
        │   scored against a rubric, every attempt anchored
        ▼                                             │
   [4]  stars accumulate → the program recomputes ────┘
        │   the predicate → the next scenario unlocks
        ▼
   [5]  a competency record that someone else can verify
        │
   [6]  they sit a real examination, and tell us what happened
        │
        └──────────────► recalibrates [4] and [5] for everyone
```

**[1] is the door and it must stay open.** No account, no gate, no medical training required. It
is also the demo: a non-clinical judge understands the product in sixty seconds. **built** (story
mode exists; the ungated first scenario is **new here**).

**[3] is the product.** 433 authored cases, 18 faculties, 671 scored runs (25 Aug 2026). **built, in Embla.**

**[4] is the mechanism nobody else has.** The unlock is not a server flag. `Progress` carries
`distinct_cases`, `level`, `attempts_counted`; `ClaimProgress` already recomputes rather than
trusts. The gate is a predicate over values the program derived itself from merkle-proven attempts.
**built for progression; the unlock predicate is specified.**

A learner sees: *I earned this, the chain checked it, the door opened.* No one has to understand
medicine to understand that.

**[6] closes the loop back onto [4] and [5].** §5.

---

## 3. The record, at three resolutions

One anchored dataset read at three zoom levels — not three products. **specified**

| Resolution | What it is | Who can mint | Stakes |
|---|---|---|---|
| **Attempt** | the anchored leaf: hash of transcript, rubric result, engine version, model id | verifier | raw evidence |
| **Progression** | level, skill tree, badge | **anyone — the program checks the maths** | low, continuous |
| **Credential** | competency attestation to a wallet | accredited issuer | high, institutional |

The learner holds the underlying data. Nothing personal goes on chain — hashes and public keys
only. What a relying party sees is what the learner chose to reveal, and they can verify it without
asking us or the learner's school.

---

## 4. The integrity stack

Each layer answers one specific way the thing could be a lie.

| Attack | Defence | State |
|---|---|---|
| Practise five times, anchor the best run | **Commit before the encounter.** The count of starts is public before any score exists | **specified — not in the program.** The headline claim with no code behind it |
| The score is whatever the vendor says | **Deterministic 40 points.** Re-run the pinned engine over the revealed transcript, get the same bytes | **built** — `vitals-sce`, `vitals-replay` |
| The judged part is a black box | **Attested, never claimed as re-derivable.** Signers state which model at which version. More signers, more confidence — never proof | **specified** |
| The learner scores themselves | Scoring runs in a verifier holding the issuer key; high-stakes attempts can require n-of-m | **specified** |
| Progression granted by a friendly server | The program recomputes the predicate from merkle proofs and writes the progression record only if its own arithmetic agrees. Nothing is minted — the record is a PDA and there is no token | **built** |
| A different keyboard, a different hash | Learner input canonicalised | **built** — `f6d6458` |
| Replay re-interprets an ambiguous order | What the order resolved to is recorded, not re-derived | **built** — `eb65f2b` |
| Two implementations quietly diverge | Conformance vectors bind them; neither depends on the other | **built** — `conformance/` |
| We quietly drop the learners who failed | Pre-registrations and payouts both on chain; anyone can count | **specified** — §5 |

**The honest limit, stated up front rather than discovered:** none of this proves the human at the
keyboard was the enrolled student. Identity binding is delegated to the issuer — school SSO,
proctoring — and the credential carries the issuer's identity so a relying party can price that
issuer's rigour themselves. A system that pretended otherwise would be lying.

---

## 5. The evidence loop — how the system learns whether it is telling the truth

**specified in `docs/OUTCOME_REPORTING.md`; the calibration use is new here.**

1. A learner **pre-registers a sitting** on chain — *"I sit examination X on date Y"* — before any
   result exists. The cohort is sealed before the outcome is known.
2. After the examination they **report what happened**, with evidence.
3. They are paid **the same amount whether they passed or failed.** The payout instruction never
   reads the outcome field, so this is enforceable from the bytecode rather than promised in a
   policy.
4. Anyone can count pre-registrations and payouts and **compute our response rate themselves.**

What this buys is not one number. It is a permanent, self-correcting link between what we measure
and what actually happens to people:

- **It sets the thresholds.** §1.
- **It ranks the cases.** A station whose score predicts real outcomes is worth more than one that
  does not, and now that is measurable per case rather than argued.
- **It bounds our own claims.** Response rate is public. A weak correlation shows up as a weak
  correlation.

This is ordinary psychometrics — with the one thing psychometrics in medical education almost never
has: **outcome labels at scale, and a public audit trail proving the sample was not curated.**

---

## 6. How 1% actually happens

A correlation is a marketing number until it changes what a learner does next. This is the part
that makes the mission a mechanism rather than a hope. **new here**

Once score predicts outcome, the system can tell a learner, before they sit:

> *Your current predicted pass probability is X. You are losing most of it in these two dimensions.
> These four stations are the ones that move them.*

That is the intervention. The mission does not happen because people practise more; it happens
because **the people most likely to fail are identified early and routed to the specific practice
that fixes their specific gap** — and because a learner who would otherwise have failed, sat again,
and quietly left medicine, does not.

Attrition at the practical examination is the
only one of the three candidate mechanisms that survives a hostile question, because the number
already exists in institutional records. This is that route, built as a feature.

**Everything needed for it already exists except the outcome labels:** `psychometrics.rs` computes
item discrimination and SEM; `competency.rs` derives per-dimension standing; the rubric is already
dimensional. The missing input is §5.

---

## 7. Where cases come from, at global scale

**82% of a case is country-independent clinical logic. 18% is language**, and about half of that
needs a native clinician rather than a translator — 25–30% for Japanese, Korean and Chinese, where
one drug appears several ways in one chart. **measured**

So a new country costs **roughly four cases of authoring work, not twenty.** Expansion is a content
cost, and it is bounded.

The differentiator this buys is worth stating plainly, because no generic simulator can copy it:
**one presentation, a different right answer per country.** Fever and rash in a child is dengue in
Jakarta, Kawasaki in Tokyo, scrub typhus in Seoul. A system that scores all three identically is
wrong in at least two countries.

**And the authors are the learners.** A learner who passed, and reported it, has demonstrated
exactly the competence that authoring requires. Passing earns the right to author; authors are paid
per attempt; their cases feed the next cohort.

That is the loop that was missing from the pipeline all along: **train → qualify → real work that
pays.** Not routing anyone to real patients — we cannot grant that standing and should never imply
otherwise — but to the work that builds the thing itself.

---

## 8. What it runs on — Asgard and Mimir are parts of the system, not tooling

Both already exist, both are AGPL, and each answers a problem stated elsewhere in this document
rather than merely supporting it.

### Asgard — the answer to §11's sustainability risk

**Asgard** is a self-hosted AI platform that runs inference entirely on local hardware with no
cloud dependency; **Heimdall** is its LLM gateway.

§11 records the risk that donation income is lumpy while inference is billed on every playthrough.
On cloud APIs that cost grows linearly with usage, forever — which is the shape of a free global
product that cannot survive donations. On Asgard it becomes **capital cost plus electricity**, and
that is the difference between a donation model that works and one that does not.

**The split is precise: cloud for the public demo, Asgard nodes for base load as they come
online.** The demo cannot wait for hardware; the base load should not stay on a meter.

And it connects to a finding from §7's economics that has nothing to do with computers: **the
faculties refused a licence purchase, but hosting a node is a different budget line with a
different approval path.** An institution that cannot buy software can often buy equipment, or
fund it from a research grant. That distributes the inference cost to institutions while the
learner stays free — which is the only arrangement that satisfies both §11's rule that the
learner never pays and the mission.

### Mimir — the content engine, and the join already exists

**Mimir** is a knowledge engine with two modes: **Curator** (PubMed → filter → chunk → embed →
vector store) and **Researcher** (GraphRAG over PrimeKG plus hybrid search).

It is already wired in by design, not by plan: `embla-cases/grounding/corpus.jsonl` is labelled in
that repository as *"corpus for grounding the Mimir agent"*. The connection was built before anyone
asked for it here.

What it makes possible is §7's differentiator. *"One presentation, a different right answer per
country"* requires the regional literature and epidemiology behind each answer — dengue in Jakarta,
Kawasaki in Tokyo, scrub typhus in Seoul — grounded rather than asserted. That is Curator and
GraphRAG doing exactly what they were built for.

⚠️ **`grounding/corpus.jsonl` contains the `hidden` exam layer and is internal.** It grounds an
agent; it is never an input to anything the learner or the public repository can reach.

### So the content bottleneck has a factory on each side

| | Produced by |
|---|---|
| Story-mode scenarios | **Skald** — §11, funded by Commissions |
| OSCE case grounding, and each new country's 18% | **Mimir** — Curator + GraphRAG |

Neither is a dependency to be acquired. Both are already running.

---

## 9. What improves without anyone deciding

Three feedback loops run continuously once §5 exists:

| Loop | Input | Effect |
|---|---|---|
| **Item quality** | item discrimination, hawk/dove examiner calibration | a case that fails to separate strong from weak learners is flagged automatically, not after a committee notices |
| **Predictive weight** | outcome correlation per case | stations that predict real outcomes rise; ones that do not are demoted or rewritten |
| **Threshold drift** | rolling correlation | badge thresholds track reality as curricula and examinations change, instead of ossifying |

The library curates itself against outcomes. That is the difference between a case bank and an
instrument.

---

## 10. Boundaries kept on purpose

These are not limitations to be removed later. Removing them breaks the system.

- **Nothing tradeable.** Progression and credentials are non-transferable by construction. A market
  in "Expert in Cardiology" would void the entire product. We forgo the volume deliberately.
- **No patient data, ever.** Virtual patients are synthetic. There is no PHI in this system to leak.
- **No learner PII on chain.** Hashes and public keys. Performance data is personal data; it stays
  off-chain, under the learner's control, revealed selectively.
- **The proof path contains no language model.** The LLM plays the patient's dialogue and is never
  anchored. Replay re-derives the outcome with no model and no keyword matcher involved at all.
- **We do not grant standing.** We can make competence provable. Only a board can make it
  authoritative. Saying otherwise would be the one lie that discredits everything else.
- **Equal payment for equal reporting.** §5.3. A validity condition, not a courtesy.

---

## 11. How it is paid for — donations

**Founder's call, 2026-08-24.** Not a fallback. It is the funding model that makes the rest of
this document internally consistent, and it changes three things.

### It removes the paywall, which the mission could not survive

*1% more doctors worldwide* is not reachable by charging medical students in the countries where
the shortage is worst. A donation-funded system is **free to the learner everywhere**, which is the
only version of §6 that reaches the people the mission is about.

It also removes every design compromise that existed to create something to sell: no gated
content beyond the earned unlocks in §2, no tiering, no reason to withhold the deterministic
scorer, and no argument for keeping `vitals-osce` closed.

### It makes our own architecture the fundraising asset

Philanthropic funders in global health have one chronic problem: **they cannot verify the impact
they paid for.** They receive reports written by the recipient.

This system produces the opposite by construction. Learner counts, attempt volume, the score →
outcome correlation, and the response rate behind it are all anchored and independently
re-derivable. A donor does not have to trust our report — they can recompute it, and so can their
auditor, and so can a rival applicant for the same money.

**The same property that makes the credential trustworthy makes the impact claim trustworthy.**
One mechanism, two beneficiaries. This is the strongest argument the project has for donation
funding and it costs nothing extra to build, because §4 and §5 already build it.

### It points at different doors than venture money

Donation funding is **non-dilutive-shaped**, and that shape is the one this design needs. Money
that expects a return eventually expects a paywall, and the first thing a paywall takes back is
the sentence above it: free to the learner everywhere. Prizes for open-source work, public-goods
and foundation grants, and global health and medical education funders all ask for the product
this document already describes rather than for a different one — which is why they are the doors
worth knocking on, and why a door that would change the product is not a better door for paying
more.

**And the institutional finding may be narrower than it looked.** Faculties declined a
*procurement*. Procurement is one budget line with one approval path. Research collaboration,
alumni funds, and CSR are different budget lines with different approvals — a faculty that cannot
buy software may still fund a study. "They will not pay" was tested; "they will not give" was not.

### Donations are visible, earmarked, and enforced — not reported

**specified — not in the program.** What runs today is the counting: the treasury is one address
anyone can read, and what this server has spent is published live on the donate page. Nothing on
chain earmarks a donation or holds it against delivery. *Coming next:* program-enforced
commissions — money given for a scenario could only be spent on that scenario, released against
the delivered content hash. The program holds no money today; that instruction is not written yet,
and everything below this line is that design rather than a description of it running.

**Founder's call, 2026-08-24: transparent donation tracking, and when a threshold is reached the
money produces the next story scenario.**

The mechanism, in the same shape as everything else in this document — the program would enforce
it, so nobody would have to trust a report.

```
Treasury      PDA · the general fund: inference, verification, operations
Commission    PDA ["comm", scenario_id] · goal · raised · state · delivered_hash
```

```
Donate { commission_id? }     into the general fund, or earmarked to one scenario
                              raised >= goal  →  state: Funded
ReleaseCommission             pays out ONLY once the scenario's content hash is registered
```

Three properties would fall out of that, and each answers a specific way donation funding normally
fails:

| Donation funding usually fails because… | Here, once the instruction exists |
|---|---|
| the donor cannot see whether the money arrived, or what it did | inflows *and* outflows would both be on chain; a donor recomputes rather than reads a report |
| "earmarked" is a promise the organisation can quietly break | **the program would refuse to spend a scenario's fund on anything else** |
| "we delivered" is asserted by the recipient | `ReleaseCommission` would require the delivered content hash — payment gated on the thing existing |

**A donor-facing page is required, not optional.** Raw chain data is public but not legible; the
public surface — raised, goal, what it funds, what shipped — is the product of this feature, and
the chain is what makes that surface impossible to fake.

### Skald is the factory that makes the threshold a real number

**built** — `asgard-skald`, a producer console driving `cutscene-gen`: an AI showrunner drafts the
series, then a per-shot loop of prompt-writer → generate → **crew review** → approve, with version
slates. It exports into story mode directly. `vitals-pitch/episodes/colosseum-5min/` already exists
as a storyboard with stills.

This matters for two reasons and they should not be confused.

**First, it makes the funding goal honest.** A scenario costs a countable number of shots times a
known generation cost, plus review. **The threshold on a Commission should be computed from actual
pipeline cost, not chosen.** A goal that was invented is a number a donor cannot check — which
would undo the entire point of putting it on chain.

**Second, it resolves the content bottleneck the unlock loop creates.** §2 gates scenarios behind
earned stars, which only works if there are scenarios to unlock; 5 of 20 specs exist. Skald is the
production line, and donations are its input. The loop closes:

```
donors fund a Commission  →  threshold reached  →  Skald produces the episode
      ▲                                                     │
      │                                                     ▼
learners who earned stars unlock it  ←──────  exported into story mode
```

Worth noting without overstating it: Skald's own design is the same principle as the rest of the
system — **a crew fails each shot before the video model sees it.** Verification before commitment,
applied to content production. It is an internal tool, not a product claim, but it is not a
coincidence either.

### The risk this introduces, stated plainly

Donations are lumpy and uncorrelated with need. A system that has to run continuously — inference
costs money on every playthrough — funded by a stream that arrives in bursts, needs either a
reserve or a recurring base before it can promise anyone permanence. **This is the open question
donation funding creates, and it should be answered before the first learner is told the service
is free forever.**

One candidate loop worth designing deliberately: a doctor who trained here, passed, and is now
earning has both the motive and the means to fund the next cohort. Alumni giving is the oldest
recurring base in education, and this system happens to know exactly who its alumni are and what
it did for them.

Two more that would come with earmarking specifically:

- **A public goal that stalls is a public failure.** Announcing a threshold would commit us to a
  number in front of everyone. That is the price of the transparency being worth anything, and it
  should be paid deliberately rather than discovered.
- **Earmarked money can strand.** Funds committed to scenario 7 could not pay the inference bill that
  keeps the service running. The general-fund/commission split has to be stated up front — donors
  choosing where money goes is the feature, and a system that can only fund scenarios while it
  starves is the failure mode.

---

## 12. What has to exist that does not yet

Ordered by how much the system depends on it.

1. **Commit before the encounter.** One instruction, one small account. The headline integrity claim
   currently has no code behind it. Everything in §4 is weaker until this exists.
2. **`vitals-osce`** — the deterministic 40 as a dependency-free crate mirroring `vitals-sce`, bound
   to Embla by its own conformance vectors. Embla's split is already clean: `score_deterministic()`
   sits beside `score_sop()`, both pure. The LLM-judged 60 stays out of the auditable core.
3. **The unlock predicate**, computed by the program.
4. **Pre-registration, outcome submission, flat payout.** §5.
5. **Psychometrics run over the 671 runs that already exist.** The code is written and has never
   been pointed at the real data. This is days of work and it is the first real number the system
   can state about itself.
6. **Attestation to an issuer**, for the credential resolution.

Items 1–3 and 5 need no permission from anyone. **5 needs no permission and no deadline** and is
the cheapest true thing available.
