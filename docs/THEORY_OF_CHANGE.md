# Theory of change — how this product produces doctors


> Written 2026-08-24. The missing link between `docs/PROBLEM_AND_SCALE.md` (the problem, measured)
> and `docs/SYSTEM_DESIGN.md` (the system, designed). It answers one question: **by what causal
> chain does a simulator turn into a practising doctor who would not otherwise exist?**
>
> Anything we cannot trace along that chain does not belong in the pitch.

---

## 0. A definition to fix first — it decides which levers count

The mission currently says **newly graduated doctors**. Two of our four problem findings sit either
side of graduation:

| Finding | Where it acts | Counts toward "graduates"? |
|---|---|---|
| Attrition 9.1%, academic difficulty 55.7% | **before** graduation | yes |
| Pass gap: 93% domestic vs 74% international | **after** graduation | **no** |

Under the strict wording, a graduate who never passes their licensing examination still counts —
and they never see a patient. That is the wrong thing to optimise.

> **Recommendation: state the mission as *newly licensed* doctors, not newly graduated.**

It is a two-word change with three consequences: it counts what actually reaches patients, it
makes **both** levers legitimate, and it matches the number the outcome loop actually measures —
we observe examination outcomes, not graduation ceremonies.

---

## 1. What we act on, and what we do not

Honesty here is what makes the rest credible.

| Problem | Do we act on it? |
|---|---|
| Attrition from **academic difficulty** (55.7% of 9.1%) | **Yes — primary lever** |
| The **19-point pass gap** for international graduates | **Yes — second lever, and the more measurable one** |
| Attrition from psychological morbidity (40%), isolation (20%), finance | **Marginally at best.** A reason to come back is not treatment. Do not claim it |
| Too few medical school places | **No.** We do not create training capacity |
| **Maldistribution** — a doctor who qualifies in Jakarta may emigrate | **No.** This is the largest thing we do not fix, and someone will ask |
| Granting standing to a credential | **No.** Only a board can do that |

Two levers. Both feed the shortage; neither solves it alone.

---

## 2. Lever one — catch the student before they fail, not after

**The failure it attacks:** a medical school discovers a student is failing *when they fail*. The
signal that would have predicted it — hundreds of scored encounters — either does not exist, or
sits unanalysed in one database.

**The four steps, and what in the system performs each:**

**1 · Remove the practice ceiling.** Students cannot practise more because standardised patients
and examiner time are scarce and expensive. A simulator removes that constraint. *This is the
enabling condition, not the mechanism* — practice volume alone is what every competitor already
offers.

**2 · Make weakness visible per dimension, continuously.** `competency.rs` derives per-dimension
standing and Dreyfus stage from attempt history; `psychometrics.rs` gives item discrimination and
standard error of measurement. The output is not "you scored 68" but *which* dimension is losing
the marks, tracked over time rather than sampled once a year.

**3 · Route to the practice that closes that specific gap.** `wfme.rs` maps rubric dimensions to
EPAs and auto-derives which EPAs a case assesses. So a weak dimension resolves to a set of stations
that exercise it. **This is the intervention.** Not "practise more" — *practise this*.

**4 · Give them a reason to come back.** The unlock loop: earned stars open the next scenario, and
the program — not a server — checks the predicate. Attrition is partly about motivation, and a
learner who returns is a learner who can be measured. Claim engagement; do not claim it treats
psychological morbidity.

---

## 3. Lever two — close the 19-point gap

International graduates pass at 74% where domestic graduates pass at 93%. Three causes, and the
product is aimed at all three:

| Cause of the gap | What the design does |
|---|---|
| **Access to preparation** | Free everywhere. This is the whole argument for donation funding — a paid product goes where people can pay, which is where the gap is smallest |
| **Format unfamiliarity** | Our stations are OSCE-shaped. Familiarity with the format is precisely what the 93% cohort has and the 74% cohort does not |
| **No scored practice against real cases** | 433 authored cases, rubric-scored, with the deterministic 40 giving feedback that is identical every time rather than dependent on which examiner was in the room |

And the differentiator matters here specifically: **one presentation, a different right answer per
country.** An international graduate sitting the USMLE needs the answer that is correct in that
system; a student in Jakarta needs the one correct there. The 82% / 18% split is what lets one
library serve both without being wrong in one of them.

---

## 4. The step that makes this not a guess

Every education product claims to improve outcomes. Almost none can show it, because they never
observe what happened afterwards.

Ours closes the loop:

```
   detect weakness per dimension
            │
            ▼
   route targeted practice
            │
            ▼
   learner sits the real examination
            │
            ▼
   outcome reported — paid the same whether pass or fail,
   cohort sealed by on-chain pre-registration
            │
            ▼
   correlation recomputed  ──────┐
            │                    │
            ▼                    │
   thresholds and routing        │
   recalibrate  ─────────────────┘
```

This is a **closed control loop**. Most of the sector is open-loop: teach, then hope.

Three things fall out of closing it, and each is a claim a competitor cannot make:

- **The detector improves.** Dimensions that predict real outcomes gain weight; ones that do not
  lose it.
- **The thresholds mean something.** A badge is the score at which observed pass probability
  crosses a stated line — not a name a committee chose.
- **The evidence itself is auditable.** Pre-registrations and payouts are both on chain, so anyone
  can confirm we did not quietly drop the learners who failed.

---

## 5. Is the required effect plausible?

The target from `PROBLEM_AND_SCALE.md` §3: attrition 9.1% → 8.2%, which is **saving about 1 in 6
of the students who currently leave for academic reasons.**

What the literature says about this class of intervention:

- Virtual patients vs traditional education: **SMD 0.90 for skills** (0.11 for knowledge) — JMIR
  systematic review and meta-analysis, 2019
- Simulation-based mastery learning, one study: **74.5% passing vs 33% in the control group**
- **The caveat that must travel with those numbers:** the same meta-analysis found large effects
  versus *passive* learning but **small or even negative** effects versus *active* learning. Our
  real comparator is not "nothing" — it is whatever the school already does

> **The required effect is smaller than the effect sizes reported for this class of intervention.
> That makes it plausible. It does not make it proven — and the study in §4 is what would settle
> it.**

Say exactly that. "Plausible, and here is the study that will decide it" is a stronger position
than a borrowed effect size presented as our own.

---

## 6. The chain, end to end

```
free access, everywhere              →  a student who could not practise, practises
practice ceiling removed             →  volume that faculty time cannot supply
weakness visible per dimension       →  the student who will fail is identified months early
routed to the stations that fix it   →  the specific gap closes, not a general one
                                     →  fewer leave for academic reasons        [lever 1]
                                     →  more pass the licensing examination     [lever 2]
                                     →  more newly LICENSED doctors
1 doctor : 1,080 people (Thailand)   →  and each one added matters more where doctors are scarcest
outcome loop                         →  and we can prove which parts of this are true
```

Each arrow is either a mechanism named in §2–§4 or a figure sourced in `PROBLEM_AND_SCALE.md`.
**If an arrow cannot be traced to one of those, it does not go in the pitch.**

---

## 7. What would falsify this

Stated deliberately, because a theory that cannot fail is not a theory:

- The correlation between anchored score and examination outcome comes back **weak**. Then the
  detector does not detect, and lever one collapses to "practice is good for you."
- Learners who use the product pass at the **same rate** as those who do not, once prior ability is
  controlled. Then we are selecting good students, not producing them.
- The gap for international graduates turns out to be driven by something we do not touch —
  language, visa, funding, discrimination in the process — rather than preparation.

All three are answerable by the study in §4. That is the strongest reason to run it early, and to
run it in a form where **we cannot quietly bury the answer.**
