# The problem, and what one doctor is worth


> Research compiled 2026-08-24 for the pitch. Every figure is **sourced**, **constructed**
> (arithmetic from sourced inputs, shown in full), or **unverified** (do not use until checked).
>
> Rule: a number that can be argued with is more persuasive than one that cannot.
>
> Second rule, added 2026-08-29 after this document was found recommending its own worst claim:
> a quantity measured **across a population** is not a quantity produced **by one member of it**.
> Do not divide an ecological association by its denominator and present the result as what one
> person does. §2 and §3 were rewritten for exactly this; §3 says why at length.

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
is barely above the United States' *primary-care-only* density (41.4 per 100,000 — Basu et al.,
2015 figure).** Name the mismatch out loud when saying it: one side is every physician, the other
is primary care alone. Said that way it is a fair and startling comparison; said without the
qualifier it is two different measures pretending to be one.

**c) About one in eleven medical students never finishes — and the top reason is fixable.**

Global average attrition **9.1%** (range 2.7–20.1%; a second review reports 11.1%).

> Causes: **academic difficulty 55.7%** · psychological morbidity 40% · absenteeism 30% ·
> social isolation 20%

Those four sum to 145.7%, so they are **not a partition** — a student who leaves can be counted
under more than one. Read 55.7% as *the most frequently cited* cause, never as "55.7% of dropouts
and nothing else". Any arithmetic that treats it as a clean slice (the 1-in-6 in §3) inherits that
looseness and must be labelled accordingly.

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

| USMLE first-time, 2022 *(no Sources entry — see §4)* | Pass |
|---|---|
| US/Canadian MD | **93%** |
| US/Canadian DO | 89% |
| **International medical graduates** | **74%** |

Step 1 IMG pass rate fell to 72% in 2023. PLAB 1 sits at 65–70%. USMLE alone tests **>100,000
candidates a year**; PLAB tested >16,000 in 2023.

**The 19-point gap is the problem statement.** Our reading of it — and it is a reading, not a
finding — is that this is not a gap in ability but a gap in access to preparation, familiarity with
format, and opportunity to practise against scored cases. That is precisely what this product
provides, and precisely the population that cannot pay for it. The gap is measured; its cause is
inferred, so say which half is which (§4).

**All four stack in the same places.** The countries with the fewest doctors have the highest
attrition and produce the graduates with the lowest pass rates.

---

## 2. What more primary care physicians are *associated* with, in years of human life

The anchor is **Basu et al., *JAMA Internal Medicine* 2019** — 3,142 US counties, 7,144 primary
care service areas, 306 hospital referral regions, 2005–2015:

> **Every 10 additional primary care physicians per 100,000 population was associated with a
> 51.5-day increase in life expectancy.**
> (Ten additional *specialists* were associated with 19.2 days — primary care is roughly 2.7× as
> strong an association.)

Also associated with each +10 PCPs per 100,000: cardiovascular mortality −0.9%, cancer mortality
−1.0%, respiratory mortality −1.4%.

### The arithmetic — **constructed**, shown in full so it can be checked

```
10 PCPs per 100,000 people    →  +51.5 days of life expectancy, each person
 × 100,000 people
                              =  5,150,000 person-days across that population
                              ÷  365.25
                              =  ~14,100 person-years across that population
                              ÷  10                (a unit conversion, not a finding)
                              =  ~1,410 person-years for each 1-per-100,000 step in density
```

The final division is where this document used to go wrong. It is arithmetic on the ratio, not a
measurement of a person, and the moment it is read aloud as "one doctor is worth 1,400 years" it
has claimed something Basu et al. does not report.

### How to state it — **use these words, they are the words already in public**

`crates/vitals-web/static/landing.html` and slide 04 of `pitch/deck.html` both carry the figure in
one agreed form. This is that form, so that the research document and the artefacts agree rather
than merely coexist:

> Across 3,142 **US** counties, every 10 additional **primary care** physicians per 100,000 people
> was **associated** with 51.5 more days of life expectancy — roughly **1,400 person-years across
> that whole population** (Basu et al., *JAMA Internal Medicine* 2019). It is a US primary-care
> association measured over populations, **not an effect any one doctor produces**, and it is not
> what we lead with.

**Why the per-doctor form was retired on 2026-08-29.** Until then this document headlined *"One
additional doctor ≈ 1,400 years of human life"* and §3 told readers to lead with it. That sentence
converts an ecological association — county-level supply correlated with county-level life
expectancy — into a causal quantity delivered by one named individual. Basu et al. measures neither
causation nor individuals. Counties with more primary care physicians also differ in income,
insurance coverage, education and rurality; the study adjusts for some of that and cannot assign
the remainder to the doctors. Any clinician who opens the citation finds this in the abstract. The
cost is not the one claim — it is that nothing else on the slide survives the discovery.

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

- **the measured gap** — one doctor per 1,080 people in Thailand, one per 2,150 in Indonesia,
  against 1:370 in Japan and 1:570 for the world (§1b). Counted, not converted. No denominator.
- **as a target** — 1% = cutting attrition by about a tenth (below). No denominator.

Use the constructed range only if someone asks directly, and label it as ours.

### The headline — **reversed 2026-08-29. Lead with the density ratios, not with 1,400.**

This section used to read "lead with the per-doctor number" and printed *"One additional doctor ≈
1,400 years of human life"* in the largest type on the page. That recommendation was wrong, and
every artefact that followed it in good faith has since been corrected: slide 04 of the deck, the
landing page, and `THEORY_OF_CHANGE.md` §6. **Do not restore it.** Lead with what was actually
counted:

> ## One doctor per **1,080** people in Thailand. One per **2,150** in Indonesia.
> Japan is **1:370**. The world average is **1:570**.

Four figures, one source each (§1b, WHO / World Bank 2019–20), with no conversion step in between.
Someone who doubts them opens the World Bank indicator and lands on the same four numbers.

**Why this is worth more than the bigger number.** Our credibility with clinicians rests on being
unusually careful about exactly this distinction — association versus effect, population quantity
versus per-person quantity. It is the same discipline the scoring engine is selling. A reader who
catches one overstated headline is entitled to discount everything behind it, including the parts
that are precisely right, and they will. 1:2,150 is a smaller number than 1,400 and a far harder
one to argue with, and the second property is the one that closes.

The 1,400 figure is not deleted and is not an embarrassment. It is a real finding from a real
study and it keeps its place in §2, in the wording §2 gives it. It is simply not a headline.

~~If a scale figure is wanted: 5,000–7,000 additional doctors × ~1,410 person-years ≈ **7–10
million years** per cohort.~~ **Withdrawn 2026-08-29.** It multiplies a denominator this section
has just declared unavailable by a per-doctor quantity §2 has just declared unsupported — a
construction resting on two things we do not have — and the honest label "on our estimate" does not
rescue a product whose other factor is wrong in kind. If a scale figure is genuinely wanted, use a
published one: WHO's shortfall of **11 million health workers by 2030** (§1a).

### And the mission is a smaller ask than it sounds

```
attrition 9.1%  (point estimate, range 2.7–20.1%)  →  100 students yield 90.9 graduates
+1% more graduates                                 →  need 91.8  →  attrition falls to 8.2%
```

> **1% = cutting medical school attrition by about one tenth.**

That identity is the durable half and it is what the deck bolds: 9.1 → 8.2 is a tenth, and "about
a tenth" survives the range moving. Carry the range with the input whenever the input is spoken —
the review that reports 9.1% gives **2.7–20.1%**, and a second review reports **11.1%** (§1c) — so
the line is offered as scale, not as a projection.

Then, in plain weight and after the caveat, never as the headline: since academic difficulty is the
most frequently cited cause at 55.7%, a tenth of attrition is **roughly 1 in 6 of the students who
currently leave because the work was too hard.** That figure is **illustrative arithmetic, not a
forecast.** It assumes attrition is the only lever, that graduates scale linearly, and that 55.7%
is a clean slice of dropouts when the reported causes overlap and sum to 145.7% (§1c). The deck and
the landing page both carry it in exactly that shape; match them.

A target a person can picture and argue with beats a percentage on its own — but only while the
picture is labelled as one.

---

## 4. The limits, stated before anyone asks

- **Basu is association, not causation**, is US data, is specific to *primary care* physicians,
  and is measured **across populations, not per physician**. Not every graduate becomes a PCP. The
  per-doctor form of this figure is retired — see §2 and §3.
- **Applying US coefficients to Indonesia is inference, not measurement.** The direction of the
  error is arguable — diminishing returns suggest the true effect where doctors are scarce is
  larger — but it is still inference and must be labelled.
- **The graduate denominator is constructed** and cannot be replaced — no such published figure
  exists. Prefer framings that do not need it.
- ~~The 42–63% figure is unverified~~ — **verified; it is a pre-reform South American cohort, not a
  developing-world average. Use the 41.5% → 3.3% form.**
- **Physician density is not associated with reduced maternal mortality** once other factors are
  controlled. Claim infant mortality only.
- **The 1-in-6 arithmetic is illustrative**, not a forecast. It assumes attrition is the only lever,
  that graduates scale linearly, and that 55.7% is a clean slice of dropouts — which it is not.
- **9.1% attrition is a point estimate inside a wide range.** The same review gives 2.7–20.1% and a
  second review reports 11.1%. Every calculation starting from 9.1% inherits that width; carry the
  range wherever the number is stated.
- **The dropout causes are not mutually exclusive.** 55.7 + 40 + 30 + 20 = 145.7%. Say "most
  frequently cited cause", not "55.7% of dropouts".
- **The USMLE and PLAB pass rates have no entry in Sources.** They are published by NBME, ECFMG and
  the GMC, but this document has not recorded which report they were read from. Cite them before
  they appear on a slide, or drop them — the same standard every other figure here is held to.
- **"Not a gap in ability, a gap in access to preparation" is our interpretation, not a finding.**
  The 19-point gap is measured; its cause is not. Present it as the reading we take, and expect to
  be asked how we would distinguish the two — the outcome loop is that answer.

---

## 5. Why this problem needs a free product, not a cheaper one

The three stacked findings — fewest doctors, highest attrition, lowest pass rates — describe the
same countries. A product that charges goes where people can pay, which is where doctors are
already plentiful and the marginal doctor stands in front of ~370 people rather than ~2,150.

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
  — **secondary source.** This is a research-unit news page reporting the WHO projection, not WHO.
  §3 now offers the 11-million shortfall as the scale figure, so replace this with the WHO health
  workforce page itself before the number is quoted in writing.
