# The problem, and what one doctor is worth


> Research compiled 2026-08-24 for the pitch. Every figure is **sourced**, **constructed**
> (arithmetic from sourced inputs, shown in full), or **unverified** (do not use until checked).
>
> Rule: a number that can be argued with is more persuasive than one that cannot.

---

## 1. The problem, in four numbers that stack

**a) The world is short of health workers, and the gap is widening.**
WHO projects a shortfall of **11 million health workers by 2030** — revised *upward* from 10
million. Over half is concentrated in Northern and sub-Saharan Africa, and most of the rest in
lower-middle-income countries. *(Health workers, not physicians. Never say "11 million doctors.")*

**b) The distribution is roughly tenfold.**

| | Physicians per 1,000 | One doctor per |
|---|---|---|
| Japan | 2.7 | ~370 people |
| South Korea | 2.6 | ~385 |
| High-income average | 2.63 | ~380 |
| **World average** | 1.75 *(17.5 per 10,000, 2019)* | **~570** |
| Low / lower-middle income | 0.91 | ~1,100 |
| **Thailand** | 0.928 *(2020)* | **~1,080** |
| **Indonesia** | 0.465 *(2019)* | **~2,150** |

WHO's threshold for universal health coverage is **4.45 physicians, nurses and midwives per
1,000** — raised from the 2006 figure of 2.3 because that older number assumed episodic maternal
care rather than the full SDG service range.

One comparison worth putting on a slide: **Indonesia's total physician density (46.5 per 100,000)
is barely above the United States' *primary-care-only* density (41.4 per 100,000).**

**c) One in eleven medical students never finishes — and the top reason is fixable.**

Global average attrition **9.1%** (range 2.7–20.1%; a second review reports 11.1%).

> Causes: **academic difficulty 55.7%** · psychological morbidity 40% · absenteeism 30% ·
> social isolation 20%

The largest single cause of losing a future doctor is academic difficulty. That is the one cause a
system which detects weakness early and routes targeted practice can act on.

### The 42–63% figure — **verified 2026-08-25, and it does not mean what it looked like**

The primary source is Rajabali, Dewji & Dewji, *Medical school dropouts: regrettable or required?*
(Imperial College London, 2018), which cites **Vergel et al.** So it is **not** a general claim
that developing countries run 42–63% attrition. It is a specific South American cohort, measured
**before** a curriculum reform.

**And the part that was cut off is the part worth having:**

> **Vergel et al. found curriculum change reduced dropout from 41.5% to 3.3%.**

State it that way. *"Developing countries lose 42–63% of medical students"* is not supported and
would be caught. *"In one South American cohort, changing how students were taught cut dropout
from 41.5% to 3.3%"* is supported, and it is **stronger for us**: direct evidence that attrition
from academic causes responds to an educational intervention, which is exactly what
`THEORY_OF_CHANGE.md` lever 1 claims. Order-of-magnitude effects are available in this space; ours
needs a tenth of one.

**d) Those who do finish do not pass equally.**

| USMLE first-time, 2022 | Pass |
|---|---|
| US/Canadian MD | **93%** |
| US/Canadian DO | 89% |
| **International medical graduates** | **74%** |

Step 1 IMG pass rate fell to 72% in 2023. PLAB 1 sits at 65–70%. USMLE alone tests **>100,000
candidates a year**; PLAB tested >16,000 in 2023.

**The 19-point gap is the problem statement.** It is not a gap in ability — it is a gap in access
to preparation, familiarity with format, and opportunity to practise against scored cases. That is
precisely what this product provides, and precisely the population that cannot pay for it.

**All four stack in the same places.** The countries with the fewest doctors have the highest
attrition and produce the graduates with the lowest pass rates.

---

## 2. What one additional doctor is worth, in years of human life

The anchor is **Basu et al., *JAMA Internal Medicine* 2019** — 3,142 US counties, 7,144 primary
care service areas, 306 hospital referral regions, 2005–2015:

> **Every 10 additional primary care physicians per 100,000 population was associated with a
> 51.5-day increase in life expectancy.**
> (Ten additional *specialists* produced 19.2 days — primary care is roughly 2.7× the effect.)

Also associated with each +10 PCPs per 100,000: cardiovascular mortality −0.9%, cancer mortality
−1.0%, respiratory mortality −1.4%.

### The arithmetic — **constructed**, shown in full so it can be checked

```
10 PCPs per 100,000 people    →  +51.5 days of life expectancy, each person
 1 PCP  per 100,000 people    →  +5.15 days,  each person
 × 100,000 people in that population
                              =  515,000 person-days
                              ÷  365.25
                              =  ~1,410 person-years
```

> ## One additional doctor ≈ **1,400 years of human life.**

### And in lower-income settings the effect is likely larger, not smaller

Diminishing returns cut in our favour. The marginal doctor in a country with one per 2,150 people
is doing work that in Japan is already being done by someone. Supporting evidence from a
cross-sectional study of UN member countries: higher physician density was associated with an
**adjusted rate ratio of 0.81 (95% CI 0.71–0.91) for infant mortality** — roughly a fifth fewer
infant deaths — after controlling for water, sanitation, governance and health spending.

Context for what is at stake in those settings: lifetime risk of maternal death is **1 in 66 in
low-income countries versus about 1 in 8,000 in high-income countries**; globally there are
~300,000 maternal deaths, 2.4 million newborn deaths and 2 million stillbirths a year.

**Honest limit, and say it before being asked:** in that same study, physician density did **not**
significantly reduce *maternal* mortality once other health-system factors were controlled. Claim
the infant-mortality association; do not claim the maternal one.

---

## 3. What 1% is worth

### The denominator — **searched again 2026-08-25 and it genuinely does not exist. Stop needing it.**

There is no published global total for medical graduates per year. Every authoritative source —
WHO GHO, OECD, Statista — reports **density per 100,000 population**, never a world sum, and the
reason given is instructive: *a high graduation rate does not automatically produce a high stock of
practising physicians, because migration, emigration, retirement and training-post bottlenecks
filter between graduation and practice.* That is the same maldistribution caveat we already carry.

Two constructions land in the same range and are recorded only so nobody redoes the work:

```
by school count   4,350+ medical schools × ~100–150 graduates      ≈ 435,000 – 650,000 / year
by density        OECD averages 13–14 per 100,000; global is lower ≈ 500,000 – 800,000 / year
```

**Recommendation: drop the "1% = N doctors" framing entirely.** It is the only claim that needs
this number, and the two stronger framings do not need it at all:

- **per doctor** — one additional doctor ≈ ~1,400 person-years (§2). No denominator.
- **as a target** — 1% = cutting attrition by about a tenth (below). No denominator.

Use the constructed range only if someone asks directly, and label it as ours.

### The headline

Lead with the per-doctor number, which is sourced and needs no denominator:

> ## One additional doctor ≈ **1,400 years of human life**

If a scale figure is wanted, it is arithmetic on our own estimate and must be labelled as such:
5,000–7,000 additional doctors × ~1,410 person-years ≈ **7–10 million years** per cohort.
**Say "on our estimate of global graduates" out loud, or do not say it.**

### And the mission is a smaller ask than it sounds

```
global attrition 9.1%  →  100 students yield 90.9 graduates
+1% more graduates     →  need 91.8  →  attrition must fall to 8.2%
```

> **1% = cutting medical school attrition by about one tenth.**
> And since academic difficulty causes 55.7% of it: **saving roughly 1 in 6 of the students who
> currently leave because the work was too hard.**

That is a target a person can picture and argue with — which is why it is more convincing than the
percentage on its own.

---

## 4. The limits, stated before anyone asks

- **Basu is association, not causation**, is US data, and is specific to *primary care*
  physicians. Not every graduate becomes one.
- **Applying US coefficients to Indonesia is inference, not measurement.** The direction of the
  error is arguable — diminishing returns suggest the true effect where doctors are scarce is
  larger — but it is still inference and must be labelled.
- **The graduate denominator is constructed** and cannot be replaced — no such published figure
  exists. Prefer framings that do not need it.
- ~~The 42–63% figure is unverified~~ — **verified; it is a pre-reform South American cohort, not a
  developing-world average. Use the 41.5% → 3.3% form.**
- **Physician density is not associated with reduced maternal mortality** once other factors are
  controlled. Claim infant mortality only.
- **The 1-in-6 arithmetic is illustrative**, not a forecast. It assumes attrition is the only lever
  and that graduates scale linearly.

---

## 5. Why this problem needs a free product, not a cheaper one

The three stacked findings — fewest doctors, highest attrition, lowest pass rates — describe the
same countries. A product that charges goes where people can pay, which is where doctors are
already plentiful and the marginal doctor is worth ~370 people rather than ~2,150.

**A paid model would aim this at the places where it matters least.** That is the argument for
donation funding, and it is a mission argument rather than an admission that we lack a revenue
model.

---

## Sources

- Basu S et al. *Association of Primary Care Physician Supply With Population Mortality in the
  United States, 2005–2015.* JAMA Intern Med, 2019 — <https://pubmed.ncbi.nlm.nih.gov/30776056/>
- WHO, *Medical doctors (per 10,000 population)* — <https://www.who.int/data/gho/data/indicators/indicator-details/GHO/medical-doctors-(per-10-000-population)>
- World Bank, *Physicians (per 1,000 people)*, SH.MED.PHYS.ZS — <https://data.worldbank.org/indicator/SH.MED.PHYS.ZS>
- WHO, *Health workforce requirements for universal health coverage* (the 4.45 threshold) — <https://apps.who.int/iris/bitstream/handle/10665/250330/9789241511407-eng.pdf>
- *Health system determinants of infant, child and maternal mortality: a cross-sectional study of
  UN member countries.* Globalization and Health — <https://www.ncbi.nlm.nih.gov/pmc/articles/PMC3247841/>
- *Factors associated with dropout in medical education: a literature review* — <https://pubmed.ncbi.nlm.nih.gov/21426375/>
- *Attrition Rate and Reasons for Attrition in Medical Schools Worldwide* — <https://www.texilajournal.com/basic-medical-sciences/article/1307-attrition-rate-and>
- OECD, *Medical graduates* — <https://www.oecd.org/en/data/indicators/medical-graduates.html>
- WHO health workforce shortage 2030 — <https://www.oucru.org/world-health-worker-week-2025/>
