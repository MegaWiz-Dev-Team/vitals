# The case factory

> Written 2026-09-16. Scope: **how Vitals World gets its patients.** The public ward does not
> run the season's content — its stations, episodes, stills, films, endings and personas stay
> with the season. World's cases come from the `embla-cases` library, compiled into the engine's
> scenario format by a deterministic tool, gated by a validator, and reviewed by a clinician
> before anything reaches production.

## What it is

`crates/vitals-casefactory` is a compiler. In: one `case.json` from `embla-cases` (433 in the
library, plus 6 endemic cases on their own branch). Out: one **pack** — a scenario `vitals-sce`
runs, a rubric `vitals-osce` marks, the patient's own words as the ward's voice, and the proof
that it was replayed. No model is called anywhere; the same case at the same content hash
compiles to the same pack on any machine.

```text
vitals-casefactory compile --cases <dir | dir@ref> --id <case_id> --out <dir>
vitals-casefactory compile --cases <dir | dir@ref> --all --out <dir>
```

`dir@ref` reads the case out of a git ref with `git show` — nothing is checked out, stashed or
written in the library. `--all` also writes `<out>/REPORT.md`. Packs live outside the repo, in
`~/.vitals/world/cases/`, like the rest of the ward's world directory.

## Archetypes — the shape a case is compiled under

A case is not converted line by line. It is fitted to one of a small library of **physiology
archetypes**: deterministic state machines with one shape of deterioration and the treatment
roles that bend it. The archetype is chosen from the case's words (diagnosis, aliases, red flags,
tags) **and gated by the vitals at presentation**: a case whose words say shock but whose systolic
is 128 is refused with the sentence *not forced*, never compiled.

| archetype | dies untreated (sim) | turned by |
|---|---|---|
| `septic_shock` | 12 min | fluids + antibiotics (+ vasopressor, + source control where the plan names them) |
| `haemorrhagic_shock` | 10 min | fluids + blood + stopping the bleed (+ the specific therapy, e.g. an antiviral) |
| `cardiogenic_shock` | 11 min | inotrope + vasopressor (+ reperfusion, + specific therapy); a fluid bolus harms |
| `neuromuscular_respiratory_failure` | 9 min | airway taken over + the antidote; a sedative before the airway harms |
| `cns_depression_hypoglycaemia` | 14 min | dextrose + the specific therapy; the airway follows the consciousness |
| `paediatric_compensated_shock` | 12 min | measured fluid, reassessed; the pulse pressure narrows first; overload harms |
| `hypoxic_respiratory_failure` | 10 min | oxygen + whichever specific therapy the plan names (bronchodilator, drain, anticoagulant, antibiotics, diuretic) |
| `anaphylaxis` | 8 min | adrenaline IM inside the window; the antihistamine-first reflex and an IV push of adrenaline are the harms |
| `acls_cardiac_arrest` | 4 min (no-flow) | the algorithm: compressions, shock for a shockable rhythm, adrenaline, amiodarone, the two-minute rhythm check, to ROSC |
| `acls_tachycardia_svt` | 13 min | vagal → adenosine; synchronised cardioversion when unstable; untreated it destabilises, arrests in VF, dies |
| `acls_tachycardia_af` | 13 min | rate control + anticoagulation; cardioversion when unstable; adenosine is the wrong drug |
| `acls_bradycardia` | 12 min | atropine, then pacing or a chronotrope; untreated it arrests in PEA |

Every archetype outside the ACLS family writes the same two-state scenario: `presenting` (deteriorating, with a
`critical` band halfway to death) → `stabilising` (improving) once **every critical role the plan
names** has been done, then a win 5 sim minutes later. The rates are derived, not tuned — each
vital moves from its starting value to the archetype's threshold in exactly the archetype's
death time — so the untreated timeline is a property of the archetype, and the reviewer reads one
number per archetype rather than one per case. The ward runs one sim second per real second on
shift and one sim minute per real hour between shifts, so these sit inside the range the season's
sixteen cases already span (3 to 14 sim minutes).

The **management plan** is read against the archetype's role table. A step that names a role's
keywords (in English or Thai) becomes a `tx_<role>` intervention; a step that forbids something
(*do not*, *avoid*, *no …*, *ห้าม*) becomes a harmful intervention with a `harm` sentence; a step
the compiler cannot place is still listed in the pack, with an empty mapping, so nothing the plan
says disappears silently. If the plan names no critical role at all, the case is refused: nothing
would turn the trajectory.

### The ACLS family — states by rhythm, moved by the clock

The four `acls_*` archetypes share one arrest core: `arrest_vf` → (shock) → `post_shock_cpr` →
(rhythm check at 2 minutes) → `rosc` or back to `arrest_vf`; `arrest_pea` and `arrest_asystole`
with their own checks (2 and 4 minutes); `rosc` → `win_icu` after the recovery time. A patient
with a pulse enters through `tachy_stable` → `tachy_unstable` → `arrest_vf`, or
`brady_unstable` → `arrest_pea`, and is turned into `converted` by the plan's critical roles. A
`perfusion` axis is the no-flow clock: it falls at 20 a minute with nobody on the chest and 4 a
minute with compressions running, and death is the floor.

The algorithm's tools — `tx_cpr`, `tx_defibrillate`, `tx_adrenaline_iv`, `tx_amiodarone`,
`tx_cardioversion` — are **rescue** roles: present in every ACLS case whether or not the plan
spells them out, never required for the turn by themselves, and each one asks which state it is
in. A shock in `arrest_vf` moves the machine; a shock in PEA, asystole or the post-shock cycle is
the harm *not shockable*; a shock into a pulse is the harm *perfusing rhythm*. The kit's own
shock button (`Step::Shock`) reaches the same edges: the engine converts VF to sinus and the
`arrest_vf` state's `{"rhythm":"sinus"}` transition takes it from there. ROSC is declared at a
rhythm check when compressions are running, adrenaline is inside its 3–5 minute window, at least
two shocks have been delivered for a shockable rhythm, and amiodarone is in once a third shock
has been needed. Nothing is drawn from a hat.

**Rhythm on the monitor.** The engine already carries a per-state `rhythm`
(`sinus|vf|vt|pea|asystole`), keys the shock button on it and lets a case key an edge on it, so
VF, PEA and asystole are drawn as themselves. What it cannot show is a *morphology for a rhythm
with a pulse*: SVT, atrial fibrillation and a heart block all read `sinus` with their rate on
the monitor, and the compiler speaks the rhythm in a beat on every change ("rhythm: ventricular
fibrillation — the pulse is gone"). A true strip for those needs an engine variable — a decision
for the ward's owner, not the compiler's.

**Gate.** Only a case whose *diagnosis* names a rhythm or an arrest enters the family; the word
"arrhythmia" in a red flag does not. A rhythm that arrives controlled (an atrial fibrillation at
88 a minute with a normal pressure) is refused as *not forced* and is never handed to another
archetype — the rhythm is the diagnosis.

## The persona is the ward's — placeholders in the prose

The ward assigns its own persona to every compiled case: a name, an age within twelve years of
the case's patient, the same sex. So the pack's prose never states the Embla patient's age or sex
as facts. `patient{age,sex}` stays as the source of truth the persona is fitted to; in the
**title**, the **presentation** (chief complaint, history, setting), every **beat**, **label** and
**harm** sentence in the scenario and the rubric, and every **voice** line, the compiler writes
placeholders the ward's renderer fills — exactly these:

| placeholder | replaces | filled with |
|---|---|---|
| `{age}` | the patient's stated age — `26-year-old`, `aged 26`, `26 years old`, `อายุ 26 ปี` | the persona's age |
| `{sex_word}` | man / woman / boy / girl / male / female / gentleman / lady; Thai `ผู้ชาย`, `ผู้หญิง`, `ผู้ป่วยชาย/หญิง`, `เด็กชาย/หญิง`, `ชายวัย…`, `หญิงอายุ…` | the persona's word for itself |
| `{he_she}` | he / she | the persona's subject pronoun |
| `{his_her}` | his / her (possessive) / hers | the persona's possessive |
| `{him_her}` | him / her (object) | the persona's object pronoun |
| `{himself_herself}` | himself / herself | the persona's reflexive |

A placeholder whose first letter is a capital — `{He_she}`, `{Sex_word}`, `{His_her}` — stood
at the start of a sentence and asks for a capitalised fill. Everything else is left as the case
wrote it, and two rules keep the honesty:

- **Only the patient's own words are replaced.** For a male patient, `she` in the text is somebody
  else and stays; for a female patient, `he` stays. A same-sex third party ("one of the boys in my
  room… he got better") is replaced too — a deterministic tool cannot tell the roommate from the
  patient, and with a same-sex persona the fill reads the same. Only the patient's own age is
  replaced; a "4-year-old son" keeps his age. Thai politeness particles (`ครับ`/`ค่ะ`) are left
  alone because the persona keeps the sex.
- **The body stays clinical.** `pregnant`, `testicular`, `menstrual` and the like are not sex words
  and are never touched; such cases are one sex by `patient.sex`.

The gate refuses a pack whose prose still carries the patient's age pattern or a sex word of the
patient's sex outside a placeholder (`REPORT.md` counts the placeholders written and the packs
refused). The plan steps and the timed sentences are quoted verbatim for the reviewer under
`management` and `timed` and are not prose the ward renders.

## What else the pack carries

- `ask_*` — one intervention per `symptom_script` line. The engine's beat is neutral
  (`history: <finding>`); the patient's **words** live in `voice`, keyed by the same id, with the
  case's own `reveal` rule (`volunteered` | `on_ask` | `on_direct_ask`). The ward assigns its own
  persona and portrait, so the Embla name is stripped everywhere and no beat carries a name.
- `exam_*` — one per non-vital examination finding; the finding is the beat.
- `ix_*` — one per expected workup entry and per investigation; the result is the beat.
- `dx_*` — the correct diagnosis, matched on its aliases.
- `vitals0` — read off the vital-sign rows in a dozen spellings; a missing saturation,
  temperature, respiratory rate or GCS is filled with a resting default **and listed under
  `vitals_assumed`**. A missing blood pressure or heart rate refuses the case.

## The rubric

Forty points, deterministic, in the shape `vitals-osce` reads:

- `action` for the plan's critical roles, the first four workup entries, up to three history
  lines the patient does not volunteer, up to three examinations the case's own criteria name, and
  the diagnosis;
- `action_by` where a red flag or plan step names a time — clamped to the moment the patient
  turns critical, because *within 1 hour* is outside a shift;
- `no_harm` for every forbidden order and for the clock;
- one `outcome` item; one `no_unindicated` item whose `allow` list is everything the case defines
  but does not pay for.

Points are split across these buckets by the case's own rubric dimension weights (the judged
`communication` dimension is dropped), then evenly within a bucket. `pass_bps` is the case's own
pass mark. The rubric's `status` string says all of this and ends *Not clinically reviewed.*

## The gate

A pack is written only if, in this order:

1. the scenario parses and validates under `vitals-sce`;
2. replayed untreated it reaches a death outcome no earlier than half and no later than twice
   the archetype's death time;
3. replayed along its recorded management path — every critical order in the plan's order,
   then everything the rubric pays for — it reaches the win with no harm recorded;
4. the rubric parses under `vitals-osce`, every needle names an intervention, an outcome or a
   harm the scenario can fire, it is out of 40, and the management path clears its own bar;
5. no string in the pack contains a season marker (`osce-`, `EP1`…`EP5`, the season's names,
   `station A`…`station D`/`OSCE station`, `/img/`, `/clip/`) or a whole-word token of the
   Embla patient's name. (An obstetric *fetal station* is not a season station.)

Before any of that, a case the library's own `deployments.jsonl` records as deployed to target
`vitals` is **refused as a season source** — the founder's rule that World never carries the
season's content — and `REPORT.md` lists them in a section of their own.

The proof — untreated death time, the winning path with its clock, the golden score — is
written into the pack under `replay`.

## Pack schema

```text
case_id, source{repo, ref, sha256}, title, country (ISO3 | null), difficulty (student|intern|resident),
clinical_tier, specialty, care_setting, language, tags[], endemic, provisional (always true),
version, archetype, archetype_label, patient{age, sex}, presentation{chief_complaint, hpi, setting} (placeholders),
placeholders{age, sex} (counts written),
sce{…vitals-sce scenario…}, rubric{case, pass_bps, status, items[]},
voice{ask_id → {finding, present, reveal, words}},
management[{step, interventions[]}], timed{role → {named_sec, by_sec, sentence}},
vitals_assumed[], replay{untreated_death_sec, win_path[{t_sec,id}], win_sec, win_outcome,
golden_score{earned,max,pass_bps}}, compiler{name, version}
```

## Refusals are the roadmap

`REPORT.md` lists every case as compiled (with its archetype) or refused (with its reason), the
archetype coverage, and the refused cases grouped by reason. The archetype library grows from the
top of that list — by adding a shape, never by stretching one over a patient it does not describe.
