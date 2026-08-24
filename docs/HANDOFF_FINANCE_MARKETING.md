# Handoff — Finance & Marketing

> ⚠️ **`docs/DECISIONS.md` is authoritative.** Parts of this file predate the decisions of
> 2026-08-24 and have not all been rewritten — where they conflict, DECISIONS.md wins.

> **Next session: this is your entry point.** Written 2026-08-24 for whoever picks up the
> financial model and the go-to-market material. Every claim below is labelled: **decided**
> (founder's call, build on it), **measured** (checked against the code), **estimate** (ours, no
> customer behind it), or **open** (nobody has resolved it).

---

## 0. Mission — CEO direction, 2026-08-24

> **"Our mission is to increase the number of newly graduated doctors worldwide by 1% per year.
> Our project will do that."**
> *(ภารกิจของเราคือการเพิ่มจำนวนหมอจบใหม่ทั่วโลกให้เพิ่มขึ้นได้ปีละ 1%)*

**Decided.** This is the frame. Everything in this document serves it, and any number that
cannot be traced back to it is decoration.

What it changes about the product's self-description: Vitals is not a training tool that happens
to be measurable. It is an attempt to **remove a throughput constraint in medical education** —
and the financial model should be built as a throughput model, not a seat-licence model.

### The mechanism the mission implies — *proposed, needs validation*

The 1% claim is directionally strong but is not yet falsifiable, and an unfalsifiable claim
collides with our own rule against pitch overclaims. Finance should pick the causal chain it can
actually defend. The candidates, in order of how defensible they look:

1. **Attrition at the practical exam.** Some share of medical students fail OSCE-type assessments
   and never re-qualify. If the product reduces that failure rate, the graduating cohort grows
   directly, and the arithmetic is a straightforward funnel.
2. **Assessment capacity.** Standardised patients and examiner time are the scarce input that caps
   how many practice encounters a school can run. Removing that cap increases readiness per
   student, which feeds (1).
3. **Cycle throughput.** Schools able to run more assessment cycles per year graduate students
   sooner, which raises the annual output without raising intake.

Route (1) is the one that survives a hostile question, because it is a number that already exists
in institutional records. **Do not claim (2) or (3) as headcount effects** — they are inputs to
(1), and presenting them as independent gains double-counts.

**First input to source:** the global annual medical-graduate figure. 1% is meaningless without a
denominator, and it must come from a citable source, not an estimate. Memory carries "4,350+
medical schools globally" from the old TAM framing — that is a school count, not a graduate
count, and it is not a substitute.

---

## 1. Read this first — three things that are now wrong

Anything you inherit from the Embla deck or from earlier Vitals docs has to be checked against
these three corrections before you use it.

### 1.1 The B2B institutional revenue line is struck — **decided 2026-08-24**

The ฿300,000–1,000,000 per institution per year band (and the ฿450,000 figure in the Embla Box
deck) is **no longer a Vitals revenue line.** MegaWiz cannot run an institutional sales motion in
another country, and Vitals is a global product, so **the buyer has to be the learner**.

~~It is still exactly what Embla-in-Thailand does.~~ **Corrected 2026-08-24: it is not.** Market
testing found that faculties do not buy at all — 18 use the product and none purchased. So the
institutional line is gone **for Embla as well as Vitals**, and with it the ฿450,000 band, the
฿0.8M→฿13M→฿52M curve and the 29%→43%→50% mix in the Embla Box deck, all of which assumed
institutional procurement.

**Do not present those figures anywhere.** What survives is the finding itself, and it is a strong
one: *"we tested institutional sales in our home market with 18 faculties actively using the
product, and nobody bought — the buyer is the learner."* Evidence-based positioning beats a plan
that has never met resistance.

### 1.2 Gross margin 86–90% is **void until recomputed** — the largest financial consequence

This is the first thing to rebuild, before any other modelling.

That margin rested on **on-prem inference**: the customer owned the appliance, so LLM cost was
near zero *to us*. A global B2C product has no appliance. **We pay for inference on every single
playthrough.** The ~฿1 per-playthrough LLM cost in the Embla deck was a cost the customer's
hardware absorbed, not a cost we carried.

Every downstream figure inherited from that deck — margin, the 29%→43%→50% B2B revenue mix, the
฿0.8M → ฿13M → ฿52M curve — assumed the institutional line and the on-prem cost base. All of it
has to be rebuilt.

**Measured, and it helps:** infrastructure is *not* the cost driver. One pinned Cloud Run instance
(the anchoring tree is in-process, so it cannot scale horizontally) plus Firestore, in
`asia-southeast1` — ~25 ms from Bangkok, ~30 ms from Jakarta. One region covers the first two
markets. The cost driver is inference, and it scales with plays, not with users.

### 1.3 "Colosseum submission is public disclosure" was wrong — **corrected 2026-08-24**

It was repeated for a day before being checked. The FAQ supports a **private repo with judges
granted access**, so submitting does **not** require publishing. `docs/RISKS.md` §2 is fixed.

The live gate is therefore about **publishing**, not **submitting**.

**On the record, and it constrains the analysis:** the repo *was* public for one day
(2026-08-22 → 2026-08-23, zero forks). That is already a disclosure event. The question in front
of counsel is not "do we disclose" but "how much more, and where does the existing day land".

---

## 2. Finance

### 2.1 "18 faculties using it" — the meaning changed, and the presentation must change with it

**Decided.** It used to be B2B traction: institutions that would buy. It is now a **distribution
channel** — access to students at effectively zero CAC.

For the pitch this is arguably *better*, because a zero-CAC channel into the exact buyer is a
harder thing to acquire than a signed institution. But it is **not a revenue line and must never
be presented as one.** If it appears in a revenue table, that is an error, not a shortcut.

### 2.2 Inputs you can feed the model today — **measured against the code**

| Input | Value |
|---|---|
| Share of a case that is country-independent clinical logic | **82%** |
| The remainder that is language | **18%**, of which ~half is keyword lists needing a **native clinician**, not a translator |
| Same figure for Japanese / Korean / Chinese | **25–30%** — CJK keyword lists run 2–3× longer, because one drug appears several ways in one chart |
| Case library | **20 specs total, shared by every market · 5 built · 15 to author** |
| Cost of opening a new market | **≈ 4 cases of authoring work** (JP/KR/CN ≈ 6), not 20 — **plus ~350–650 outcome labels, see below** |

The 82% is what turns "author in Thailand, ship worldwide" from aspiration into arithmetic: **a new
country costs the language packs, not the library.** That last row is the single most important
line in this table for anyone modelling international expansion — market entry is a content cost
of a handful of cases, and it is bounded.

Note the native-clinician requirement is a *cost quality* issue, not just a rate issue: half of the
18% cannot be bought at translator prices. Model it as clinical labour.

**⚠️ Market entry has a second cost that earlier drafts of this document omitted: data.** The
pass-probability model is calibrated **per examination**, not globally — a model fitted to
Thailand's ศรว. cannot predict Indonesia's UKMPPD, because the threshold, the content and the
candidate population all differ.

Rule of thumb for a clinical prediction model is 10–20 events per predictor. With ~5 predictors
and a ~15% failure rate that is **~350–650 learners with reported outcomes, per examination**,
before the prediction feature works in that market — which takes at least one full examination
cycle of that country.

So a new market is **4 cases *and* one outcome cycle**, not 4 cases. Model the two separately: the
content cost is small and immediate, the data cost is small and *slow*, and it is the slow one
that sets when the market is actually served.

### 2.3 What survives from the old revenue model — **estimates, no signed customer**

With line 2 struck, five streams remain. Sequenced short → long:

1. **Prize and accelerator money** *(Colosseum's published terms — the only non-estimate here)*
2. **Pharma / MedTech sponsorship** — sponsored clinical episodes; outcome-linked grants
3. **Scenario marketplace take-rate** — 10–20% per replay to the platform
4. **Verification / credential fees** — $10–50 per deep verification by employers and residency
   programmes abroad
5. **B2C packs for doctors and students** — exam scenario packs, CME certificates

Streams 2–5 are the founder's own estimates with no signed customer behind them. **Label them as
estimates in any deck.** With B2B gone, stream 5 carries the near-term weight and is where the
inference-cost problem from §1.2 bites hardest — price it after the margin rebuild, not before.

---

## 3. Marketing

### 3.1 Hard constraints — these are not preferences

**Decided, and they bind today.**

- **Superseded 2026-08-24.** The patent gate was closed by founder's decision: the patent is not the moat — the case library, the faculties already using it, the conformance suite and the accumulating outcome data are. The repository opens and a public demo is now wanted. See `docs/TECH_HANDOFF_PROMPT.md` and `docs/CHECKLIST.md` §A.
- **A public demo is now part of the plan**, with rate limits and a spend ceiling. Marketing may point at it.
- **Submitting to Colosseum is fine.** Private repo, judges granted access. Nothing about the main
  award requires publishing.

### 3.2 The Public Goods Award is a **trade**, not free ammunition

**Decided — and this reverses an earlier recommendation in `docs/PRECEDENT_FRONTIER.md` §5, which
was written before the patent gate was factored in.**

It looked like a second shot at a prize for no product change. It is not, because it almost
certainly requires a **public repo**.

The trade, stated plainly:

| Gain | Cost |
|---|---|
| ~$10,000 and a far less contested door (Zoneless won it *and* advanced to Cohort 5) | **Novelty in three of four target markets** |

Europe, China, Japan and Korea apply **absolute novelty with no grace period**. The US allows 12
months. JP/KR/CN were added to the market plan *after* `RISKS.md` §2 was written, which is how the
conflict arose.

The main award needs no such trade. **Treat the Public Goods Award as a decision for the founder
and counsel, not as an item on a marketing checklist.**

### 3.3 Positioning that came out of the work

Lead with the **mechanism, not the sector**: *a skill nobody can currently prove, made provable
and portable across borders.* "Training platform for medical students" gives this audience no
frame; "a market for verifiable skill" gives them the same frame that made the robotics-DePIN
project win the same hackathon.

Two claims are now **literally true in code**, not aspirational — use them, they are rare:

- replay re-derives an outcome with **no language model and no keyword matcher involved at all**
- a learner who changes phones or keyboards still produces **the same hash**

### 3.4 The differentiator worth building the campaign on

**One presentation, a different right answer per country.** Fever and rash in a child is dengue in
Jakarta, Kawasaki in Tokyo, scrub typhus in Seoul.

This is what the 82%/18% split buys, it is defensible against any generic-simulator competitor,
and it is the clearest consumer-legible reason the product has to be global rather than a Thai
product sold abroad.

---

## 4. The unresolved conflict — **open, founder + counsel, not a spreadsheet**

**Our strongest-demand markets are our worst patent markets.**

Japan and Korea plausibly have mandatory national practical examinations — **unverified, and worth
confirming early**, because if true it is built-in compelled demand of exactly the kind that is
hardest to find. They are also absolute-novelty jurisdictions with no grace period.

Nobody has taken this to a lawyer yet. Two things can be done in parallel while that is pending,
and both are cheap:

1. **Confirm whether the JP/KR mandatory practical exams exist**, and their scale. This is desk
   research, it is not blocked by counsel, and it decides how much the novelty is worth giving up.
2. **Establish where the one-day public window (2026-08-22 → 08-23) leaves us.** It is already on
   the record, so it is an input to the legal question rather than a thing to avoid mentioning.

Do not model JP/KR revenue as committed until (1) comes back.

---

## 5. Where things live

| | |
|---|---|
| Product & market design, engineering inputs | this file, plus `docs/PRECEDENT_FRONTIER.md` |
| Hackathon field analysis, competitor precedent, prior decisions | `docs/PRECEDENT_FRONTIER.md` |
| Patent gate, disclosure sequencing | `docs/RISKS.md` §2 |
| Protocol design | `docs/ARCHITECTURE.md` · `docs/TOKENOMICS.md` · `docs/GAMIFICATION.md` |
| What a second implementation needs | `conformance/` |
| Embla's Thai institutional model (**still valid for Embla**) | `megawiz-pitch/ymid-embla-box/embla-box-deck-v4.html` slide 7 |

**Standing rules that apply to everything above:** estimates get labelled as estimates; value is
framed as time saved rather than headcount replaced; no claim goes in a deck that the code cannot
support.
