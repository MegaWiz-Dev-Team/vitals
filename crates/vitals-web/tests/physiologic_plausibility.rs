//! Every number this engine can put on a screen, against the patient it claims to belong to.
//!
//! We shipped `58/58` — a systolic equal to its diastolic, on a conscious talking patient — and
//! nobody noticed until a producer looked at a screenshot. The sweep that followed found two
//! different things, and reporting them as one number overstates the serious half more than
//! threefold:
//!
//!   * **308 readings with no pulse pressure at all**, across **five** of the seventeen cases —
//!     `ep1`, `ep2`, `ep4`, `ep5`, `osce-a`. Every one of the 308 was exactly zero, not merely
//!     narrow. That is the impossible count, and it is the one that matters.
//!   * **981 readings outside the plausible band**, across **seven** cases — those five plus
//!     `osce-d` and `osce-d3`. It is the 308 above plus 673 more that were narrower than 10 mmHg,
//!     which is implausible for this shelf rather than impossible for a body.
//!
//! The commit titled *"a pressure of 58/58 is not a low one, it is not a pressure"* fixed the
//! cause. Both figures are this file's own output rather than a recollection of it, and they are
//! re-measurable: check that commit's parent out into a worktree, drop this file in, and run it.
//!
//! This file is the net for the next one, and it is built on the principle that made the first one
//! so expensive: a clinician who sees one impossible number
//! stops believing every number, so an impossible number must never be something a human has to
//! spot.
//!
//! ## What it walks
//!
//! Every case the shelf ships — the twelve OSCE stations, the four season episodes and the
//! conformance copy of EP1 — a tick at a time from `vitals0` to its terminal state, along three
//! paths:
//!
//!   * **untreated** — nobody does anything. The trace that produced `58/58`.
//!   * **treated** — every intervention the case declares *except* the ones it marks as harm.
//!     The path a candidate who knows the case takes, and the only one that reaches the
//!     `improving` / `recovering` half of most automatons.
//!   * **everything** — every intervention including the harmful ones. Blunt on purpose: it is
//!     the cheapest way to reach the states an author never walked, and it is a path a real
//!     learner can take, because ordering IV-push adrenaline is one keystroke.
//!
//! Fifty-one runs and a little over twenty thousand ticks, in well under a tenth of a second.
//! Nothing here opens a socket, a database or a clock, so there is no reason to skip it. Each
//! check prints how much it examined and fails if that collapses, because a sweep that walked
//! nothing passes everything.
//!
//! ## What it asserts, and what it merely reports
//!
//! The line is **physical impossibility versus clinical judgement**. A pulse pressure of zero is
//! not a severe blood pressure, it is not a blood pressure — that is an assertion. Whether a man
//! reperfused at a systolic of 88 should be going home is a question for a doctor — that is a
//! line in the report, not a bound in a test. Where a threshold had to be chosen it is cited,
//! and it is chosen wide: a false alarm costs an investigation, a missed impossibility costs a
//! clinician's trust in every number on the page.
//!
//! ## Where the numbers come from
//!
//!   * **Ages** are read out of `crates/vitals-web/src/main.rs`'s `AGES` table at run time, not
//!     copied. A second copy is a copy that drifts, and the drift would land on the five
//!     paediatric cases — `osce-b3` (3), `ep3` (5), `osce-c` (6), `osce-d3` (6), `osce-b2` (14)
//!     — where a normal adult heart rate is a bradycardia.
//!   * **Age-banded normal ranges** for HR, RR and systolic are the same APLS/PALS-shaped bands
//!     the bedside monitor already alarms on (`static/device/monitor.html`, `AGE_BANDS`). They
//!     are repeated here rather than parsed because that file is a page whose markup is free to
//!     change; the numbers are quoted in the table below so the two can be read side by side.
//!   * **SpO₂ ≥ 94, temperature 36.1–38.0 and GCS 15** are the windows the published NEWS2 table
//!     scores zero for (`vitals_web::news2`).
//!   * **The survivable envelope** — the outer limits below — is deliberately far wider than any
//!     of those. It is not a claim about what is normal. It is the line past which a number is
//!     not a sick patient but a broken model.
//!
//! Run `cargo test -p vitals-web --test physiologic_plausibility -- --nocapture` for the census
//! tables.

use std::collections::BTreeMap;
use std::path::PathBuf;
use vitals_sce::runtime::{Rhythm, ShockResult};
use vitals_sce::{PatientStatus, Sce, SceState, Vitals};
use vitals_web::reading::Reading;

// ── the shelf ───────────────────────────────────────────────────────────────

/// Every case this server can be asked to play, in shelf order — the same seventeen
/// `main.rs::every_case()` lists.
const CASES: [&str; 17] = [
    "ep1", "ep2", "ep3", "ep4", "ep5", "osce-a", "osce-a2", "osce-b", "osce-b2", "osce-b3",
    "osce-c", "osce-c2", "osce-c3", "osce-d", "osce-d2", "osce-d3", "osce-d4",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Mirrors `main.rs::scenario_path`, arm for arm. A case whose file this cannot find fails the
/// suite loudly rather than being quietly dropped from the sweep — the sweep is the deliverable.
fn case_path(id: &str) -> PathBuf {
    let r = repo_root();
    match id {
        "ep1" => r.join("conformance/sce-anaphylaxis-ep1.json"),
        "ep2" => r.join("demo/scenarios/ep2-stemi.json"),
        "ep3" => r.join("demo/scenarios/ep3-epiglottitis.json"),
        "ep4" => r.join("demo/scenarios/ep4-pulmonary-embolism.json"),
        "ep5" => r.join("demo/scenarios/ep5-the-night-the-stars-fell.json"),
        s => r.join("demo/stations").join(format!("{s}.sce.json")),
    }
}

fn load(id: &str) -> Sce {
    let p = case_path(id);
    let json = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    Sce::from_json(&json).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

// ── ages, read from the one place that holds them ───────────────────────────

/// The `AGES` table out of `main.rs`, parsed from source.
///
/// It is private to a binary, so a test cannot call `patient_age`. The alternative to reading it
/// is writing the seventeen ages down a second time, and a second copy of a fact is a copy that
/// drifts — silently, and onto the children, which is the direction that costs the most. Parsing
/// the source keeps one authority for the fact and turns a rename into a loud failure here
/// instead of a quiet disagreement between a monitor and a score.
fn ages() -> BTreeMap<String, f64> {
    const MARKER: &str = "const AGES: &[(&str, f64)] = &[";
    let path = repo_root().join("crates/vitals-web/src/main.rs");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let start = src.find(MARKER).unwrap_or_else(|| {
        panic!("`{MARKER}` is no longer in main.rs — the ages moved, and this reader has to move with them")
    });
    let table = &src[start + MARKER.len()..];
    let table = &table[..table.find("];").expect("unterminated AGES table")];

    let mut out = BTreeMap::new();
    for line in table.lines() {
        let l = line.trim();
        let Some(row) = l.strip_prefix('(') else { continue };
        let Some(row) = row.split(')').next() else { continue };
        let mut it = row.split(',');
        let (Some(id), Some(yrs)) = (it.next(), it.next()) else { continue };
        let id = id.trim().trim_matches('"').to_string();
        let yrs: f64 = yrs.trim().parse().unwrap_or_else(|e| panic!("AGES row {l:?}: {e}"));
        out.insert(id, yrs);
    }
    assert_eq!(out.len(), CASES.len(), "AGES no longer covers exactly the shelf: {out:?}");
    out
}

// ── what is normal, and what is merely possible ─────────────────────────────

/// One age band: the range the bedside monitor treats as normal, and the far wider range outside
/// which the number is not a patient at all.
///
/// `normal` is `static/device/monitor.html`'s `AGE_BANDS` verbatim — the APLS/PALS-shaped ranges
/// a resuscitation chart carries, and the ones the screen already prints beside each number.
///
/// `possible` is this file's own, and it is wide on purpose. A three-year-old in an SVT runs at
/// 220; a septic adult in a compensating tachycardia runs at 180; a child in decompensated shock
/// can be bradycardic at 40 with a pulse you can still feel. None of those is a bug. What is a
/// bug is a heart rate of 4, or of 900, or a systolic of 12 on a patient the engine says is
/// talking — numbers no body produces, which is exactly what an unguarded `rate_per_min` walking
/// past its floor will produce.
///
/// The systolic floor is 30 at every age and not banded, because the bottom of that axis is not
/// age-dependent: a systolic of 30 is agonal in a toddler and in a seventy-year-old alike, and
/// EP5 legitimately runs an exsanguinating adult down to 45 in the seconds before the arrest.
/// The *ceilings* are banded, because those are the ones that catch a dynamic with no `ceil`.
struct Ages {
    label: &'static str,
    /// Upper bound of the band, exclusive, in years.
    under: f64,
    hr: (f64, f64),
    rr: (f64, f64),
    sbp: (f64, f64),
    hr_possible: (f64, f64),
    rr_possible: (f64, f64),
    sbp_possible: (f64, f64),
}

const BANDS: &[Ages] = &[
    Ages { label: "INFANT",   under: 1.0,  hr: (100.0, 160.0), rr: (30.0, 60.0), sbp: (70.0, 100.0),
           hr_possible: (40.0, 260.0), rr_possible: (8.0, 90.0), sbp_possible: (30.0, 140.0) },
    Ages { label: "1-2 YR",   under: 3.0,  hr: (90.0, 150.0), rr: (24.0, 40.0), sbp: (80.0, 110.0),
           hr_possible: (35.0, 250.0), rr_possible: (6.0, 80.0), sbp_possible: (30.0, 150.0) },
    Ages { label: "3-5 YR",   under: 6.0,  hr: (80.0, 140.0), rr: (22.0, 34.0), sbp: (85.0, 110.0),
           hr_possible: (30.0, 240.0), rr_possible: (6.0, 70.0), sbp_possible: (30.0, 170.0) },
    Ages { label: "6-11 YR",  under: 12.0, hr: (70.0, 120.0), rr: (18.0, 30.0), sbp: (90.0, 120.0),
           hr_possible: (30.0, 230.0), rr_possible: (5.0, 65.0), sbp_possible: (30.0, 190.0) },
    Ages { label: "12-17 YR", under: 18.0, hr: (60.0, 100.0), rr: (12.0, 20.0), sbp: (100.0, 130.0),
           hr_possible: (25.0, 220.0), rr_possible: (5.0, 60.0), sbp_possible: (30.0, 230.0) },
    Ages { label: "ADULT",    under: f64::INFINITY, hr: (50.0, 120.0), rr: (8.0, 24.0), sbp: (90.0, 160.0),
           hr_possible: (20.0, 220.0), rr_possible: (4.0, 60.0), sbp_possible: (30.0, 270.0) },
];

fn band(age: f64) -> &'static Ages {
    BANDS.iter().find(|b| age < b.under).unwrap_or(&BANDS[BANDS.len() - 1])
}

/// Saturation and temperature do not band by age — 94% is 94% at three and at seventy — so they
/// take the windows the published NEWS2 table scores zero for (`vitals_web::news2`).
const SPO2_NORMAL: (f64, f64) = (94.0, 100.0);
const TEMP_NORMAL: (f64, f64) = (36.1, 38.0);

/// A saturation a pulse oximeter can read off a patient who still has a pulse. Cyanotic
/// congenital disease lives in the fifties for years; below thirty there is no reading, only a
/// probe with nothing to measure.
const SPO2_POSSIBLE: (f64, f64) = (30.0, 100.0);
/// Core temperature. Survival is recorded either side of both of these; a case outside them is
/// not a patient, it is an unclamped dynamic.
const TEMP_POSSIBLE: (f64, f64) = (25.0, 43.0);

/// The gap between the two pressures.
///
/// Zero is arithmetically impossible in anything with a heartbeat and is the reading this whole
/// file exists because of. The outer bound is loose — a wide pulse pressure is a real finding in
/// aortic regurgitation and in sepsis — and catches only a diastolic that has come unstuck from
/// its systolic in the other direction.
const PP_IMPOSSIBLE: (f64, f64) = (0.0, 120.0);
/// Tighter, and still generous: a pulse pressure under 10 is seen in tamponade and in end-stage
/// cardiogenic shock, so this is a flag rather than an impossibility — but no case on this shelf
/// is authored to teach either, so one appearing is a stuck diastolic until proven otherwise.
const PP_IMPLAUSIBLE: (f64, f64) = (10.0, 100.0);

// ── one walk ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Path {
    Untreated,
    Treated,
    Everything,
}

impl Path {
    fn name(self) -> &'static str {
        match self {
            Path::Untreated => "untreated",
            Path::Treated => "treated",
            Path::Everything => "everything",
        }
    }
}

const PATHS: [Path; 3] = [Path::Untreated, Path::Treated, Path::Everything];

/// The seven vitals, in the order every table here prints them.
const VITALS: [&str; 7] = ["hr", "sbp", "dbp", "spo2", "rr", "temp", "gcs"];

fn axes(v: &Vitals) -> [f64; 7] {
    [v.hr, v.sbp, v.dbp, v.spo2, v.rr, v.temp, v.gcs as f64]
}

/// One tick, as both the model holds it and a monitor would print it.
#[derive(Clone, Copy)]
struct Sample {
    t: u32,
    v: Vitals,
    r: Reading,
    status: PatientStatus,
    /// The run has reached a terminal outcome at or before this tick.
    ended: bool,
}

struct Run {
    case: &'static str,
    path: Path,
    age: f64,
    samples: Vec<Sample>,
    outcome: Option<String>,
    /// True if the walk hit the tick limit with the case still running.
    open: bool,
}

impl Run {
    fn at(&self, s: &Sample) -> String {
        format!("{}/{} t={}s", self.case, self.path.name(), s.t)
    }
    /// Every tick at which the patient is alive and the heart is producing output — the only
    /// ticks on which a blood pressure or a saturation means anything.
    fn perfusing(&self) -> impl Iterator<Item = &Sample> {
        self.samples.iter().filter(|s| !s.ended && s.r.pulse)
    }
    fn alive(&self) -> impl Iterator<Item = &Sample> {
        self.samples.iter().filter(|s| !s.ended)
    }
}

/// Half an hour of sim time. Every case on the shelf either terminates or has been shown to be
/// standing still long before this; the two that stand still are a finding of their own.
const LIMIT_SEC: u32 = 1800;

fn walk(case: &'static str, path: Path, age: f64) -> Run {
    let sce = load(case);
    let mut st = SceState::new(sce.clone());

    // Orders are given up front and the clock then runs undisturbed, so that everything moving
    // during the walk is the case's own dynamics and triggers. A direction-of-change check on a
    // trace where a learner is also pushing fluids measures the learner, not the scenario.
    if path != Path::Untreated {
        st.tick(10.0);
        for iv in &sce.interventions {
            if path == Path::Treated && iv.harm.is_some() {
                continue;
            }
            st.apply_id(&iv.id);
        }
    }

    let mut samples = Vec::with_capacity(LIMIT_SEC as usize);
    let mut open = true;
    for t in 1..=LIMIT_SEC {
        st.tick(1.0);
        let v = st.vitals;
        samples.push(Sample {
            t,
            v,
            r: Reading::of(&v).rounded(),
            status: st.status,
            ended: st.outcome().is_some(),
        });
        if st.outcome().is_some() {
            open = false;
            break;
        }
    }
    Run { case, path, age, samples, outcome: st.outcome_id().map(str::to_string), open }
}

fn every_run() -> Vec<Run> {
    let ages = ages();
    let mut out = Vec::new();
    for case in CASES {
        let age = *ages.get(case).unwrap_or_else(|| panic!("{case} has no age in main.rs's AGES"));
        for p in PATHS {
            out.push(walk(case, p, age));
        }
    }
    out
}

// ── reporting ───────────────────────────────────────────────────────────────

/// Fail with every distinct finding named, and never with a wall of the same fact.
///
/// A stuck diastolic is wrong on every one of 1800 ticks. Printing all of them buries the second
/// bug under the first, and a failure nobody reads is a failure nobody fixes — so this keeps the
/// first occurrence per (case, path, kind) and counts the rest.
/// A sweep that walked nothing passes every assertion in this file, and looks identical to one
/// that walked everything. So each test says out loud how much it actually looked at, and fails
/// if that number collapses — a suite whose coverage can quietly go to zero is worse than none,
/// because it is trusted.
fn examined(what: &str, n: usize, least: usize) {
    println!("  examined {n} {what}");
    assert!(
        n >= least,
        "only {n} {what} were examined (expected at least {least}) — this check has gone vacuous, \
         which looks exactly like passing"
    );
}

fn report(headline: &str, findings: Vec<(String, String)>) {
    if findings.is_empty() {
        return;
    }
    let mut first: Vec<&(String, String)> = Vec::new();
    for f in &findings {
        if !first.iter().any(|(k, _)| *k == f.0) {
            first.push(f);
        }
    }
    let body = first.iter().map(|(_, m)| format!("  {m}")).collect::<Vec<_>>().join("\n");
    panic!(
        "{headline}\n{} finding(s) over {} distinct case/path/kind; first of each:\n{body}",
        findings.len(),
        first.len()
    );
}

// ── 1. arithmetic ───────────────────────────────────────────────────────────

/// Before anything clinical: the numbers have to be numbers.
///
/// NaN and infinity are what an unguarded division or a runaway `rate_per_min` leave behind, and
/// both of them compare false against every bound below — so a suite that checked ranges first
/// would pass a screen full of `NaN`. A negative rate and a saturation of 104% are the same class
/// of defect one step further along.
#[test]
fn every_number_the_engine_emits_is_a_number_a_meter_could_show() {
    let mut bad = Vec::new();
    let mut seen = 0usize;
    for run in every_run() {
        for s in &run.samples {
            seen += 1;
            let key = |k: &str| format!("{}/{}/{k}", run.case, run.path.name());
            for (name, x) in VITALS.iter().zip(axes(&s.v)) {
                if !x.is_finite() {
                    bad.push((key(name), format!("{} {name}={x} — not a finite number", run.at(s))));
                }
            }
            let v = s.v;
            if v.hr < 0.0 {
                bad.push((key("hr<0"), format!("{} hr={:.1} — a negative rate", run.at(s), v.hr)));
            }
            if v.rr < 0.0 {
                bad.push((key("rr<0"), format!("{} rr={:.1} — a negative rate", run.at(s), v.rr)));
            }
            if v.sbp < 0.0 || v.dbp < 0.0 {
                bad.push((key("bp<0"), format!("{} bp={:.1}/{:.1} — a negative pressure", run.at(s), v.sbp, v.dbp)));
            }
            if !(0.0..=100.0).contains(&v.spo2) {
                bad.push((key("spo2"), format!("{} spo2={:.1}% — a saturation is a fraction of haemoglobin", run.at(s), v.spo2)));
            }
            // 3 is the floor of the Glasgow scale: eyes 1, voice 1, motor 1. There is no 2.
            // `SceState::set_var` clamps `gcs` to 0..=15, which leaves 0, 1 and 2 reachable by an
            // authored `set` — see `the_glasgow_coma_scale_has_no_score_below_three`.
            if !(3..=15).contains(&v.gcs) {
                bad.push((key("gcs"), format!("{} gcs={} — the scale runs 3 to 15", run.at(s), v.gcs)));
            }
        }
    }
    examined("ticks over the whole shelf", seen, 10_000);
    report("the engine emitted a number no meter could show", bad);
}

// ── 2. a living patient ─────────────────────────────────────────────────────

/// A patient with a pulse has a rate and a pressure, and the pressure has two different numbers.
///
/// This is the `58/58` invariant, generalised and moved onto what the *monitor* reports rather
/// than what the model holds: `Reading` is the one place that decides whether there is a
/// measurement to be had at all, so checking there is checking what a clinician sees.
#[test]
fn a_patient_with_a_pulse_has_a_rate_and_a_pressure() {
    let mut bad = Vec::new();
    let mut seen = 0usize;
    for run in every_run() {
        let key = |k: &str| format!("{}/{}/{k}", run.case, run.path.name());
        for s in run.perfusing() {
            seen += 1;
            let (Some(sbp), Some(dbp)) = (s.r.sbp, s.r.dbp) else {
                bad.push((key("no-bp"), format!("{} pulse, and no blood pressure to read", run.at(s))));
                continue;
            };
            if s.r.hr <= 0.0 {
                bad.push((key("hr0"), format!("{} hr={:.0} on a perfusing rhythm", run.at(s), s.r.hr)));
            }
            if s.r.rr <= 0.0 {
                bad.push((key("rr0"), format!("{} rr={:.0} — apnoea on a patient with a pulse", run.at(s), s.r.rr)));
            }
            if sbp <= 0.0 {
                bad.push((key("sbp0"), format!("{} sbp={sbp:.0} on a perfusing rhythm", run.at(s))));
            }
            if s.r.spo2.is_none() {
                bad.push((key("no-spo2"), format!("{} pulse, and no saturation to read", run.at(s))));
            }
            if dbp >= sbp {
                bad.push((key("inverted"), format!(
                    "{} {sbp:.0}/{dbp:.0} — the diastolic is not below the systolic", run.at(s))));
            }
        }
    }
    examined("ticks with a pulse", seen, 10_000);
    report("a patient the engine says has a pulse did not read like one", bad);
}

/// The gap between the two pressures, on every tick of every path.
///
/// `pulse_pressure.rs` pins this for the untreated and fully-treated walks; this repeats it on
/// the third path and against the monitor's rounded numbers, because `58.4/57.6` prints as
/// `58/58` and it was the print that was reported.
#[test]
fn every_blood_pressure_has_a_pulse_pressure() {
    let mut bad = Vec::new();
    let mut seen = 0usize;
    for run in every_run() {
        let key = |k: &str| format!("{}/{}/{k}", run.case, run.path.name());
        for s in run.perfusing() {
            let (Some(sbp), Some(dbp)) = (s.r.sbp, s.r.dbp) else { continue };
            seen += 1;
            let pp = sbp - dbp;
            if pp <= PP_IMPOSSIBLE.0 {
                bad.push((key("pp<=0"), format!("{} {sbp:.0}/{dbp:.0} pp={pp:.0} — not a low pressure, not a pressure", run.at(s))));
            } else if pp >= PP_IMPOSSIBLE.1 {
                bad.push((key("pp>=120"), format!("{} {sbp:.0}/{dbp:.0} pp={pp:.0} — the diastolic has come unstuck", run.at(s))));
            } else if pp < PP_IMPLAUSIBLE.0 {
                bad.push((key("pp<10"), format!("{} {sbp:.0}/{dbp:.0} pp={pp:.0} — below any band this shelf teaches", run.at(s))));
            } else if pp > PP_IMPLAUSIBLE.1 {
                bad.push((key("pp>100"), format!("{} {sbp:.0}/{dbp:.0} pp={pp:.0} — above any band this shelf teaches", run.at(s))));
            }
        }
    }
    examined("blood pressures", seen, 10_000);
    report("a blood pressure reached the screen with no pulse pressure in it", bad);
}

// ── 3. a dead patient ───────────────────────────────────────────────────────

/// Death agrees with itself across every field at once.
///
/// The audit found `HR 0 · SpO₂ -- · BP -- · RR 0 · ASYSTOLE` and that agreement is worth as much
/// as the numbers are: the failure it replaced was a patient marked `Dead` with a pulse of 128
/// and a respiratory rate of 28, the monitor sweeping and the chest still rising. Pinned here so
/// the next change to `terminate` cannot loosen one field and leave the other six saying
/// otherwise.
#[test]
fn a_dead_patient_is_dead_in_every_field_at_once() {
    let mut bad = Vec::new();
    let mut deaths = 0;
    for run in every_run() {
        let Some(last) = run.samples.last() else { continue };
        if !last.ended {
            continue;
        }
        let is_death = run.outcome.as_deref().is_some_and(|o| o.starts_with("death"));
        if !is_death {
            continue;
        }
        deaths += 1;
        let key = |k: &str| format!("{}/{}/{k}", run.case, run.path.name());
        let v = last.v;
        let r = last.r;
        let mut wrong: Vec<String> = Vec::new();
        for (name, x) in ["hr", "sbp", "dbp", "spo2", "rr"].iter().zip([v.hr, v.sbp, v.dbp, v.spo2, v.rr]) {
            if x != 0.0 {
                wrong.push(format!("{name}={x:.1}"));
            }
        }
        if v.gcs != 3 {
            wrong.push(format!("gcs={}", v.gcs));
        }
        if v.rhythm != Rhythm::Asystole {
            wrong.push(format!("rhythm={}", v.rhythm.as_str()));
        }
        if last.status != PatientStatus::Dead {
            wrong.push(format!("status={:?}", last.status));
        }
        if r.pulse || r.spo2.is_some() || r.sbp.is_some() || r.dbp.is_some() || r.shockable {
            wrong.push(format!("the monitor still reads pulse={} spo2={:?} bp={:?}", r.pulse, r.spo2, r.sbp));
        }
        if !wrong.is_empty() {
            bad.push((key("dead"), format!(
                "{} outcome={:?} but {}", run.at(last), run.outcome, wrong.join(", "))));
        }
    }
    assert!(deaths >= 12, "only {deaths} of the runs reached a death — the sweep is not walking the whole shelf");
    report("a patient the engine killed was not dead in every field", bad);
}

// ── 4. age ──────────────────────────────────────────────────────────────────

/// A vital sign belongs to a patient of a particular age, and five of these patients are children.
///
/// The bounds are the survivable envelope, not the normal band: a frightened three-year-old at
/// 180 is not a defect and must not be reported as one. What this catches is the class the
/// respiratory rate nearly gave us — an axis driven by an adult-shaped rate on a child, walking
/// past anything a body of that size does.
#[test]
fn every_reading_is_possible_for_a_patient_of_that_age() {
    let mut bad = Vec::new();
    let mut seen = 0usize;
    for run in every_run() {
        let b = band(run.age);
        let key = |k: &str| format!("{}/{}/{k}", run.case, run.path.name());
        for s in run.perfusing() {
            seen += 1;
            let outside = |x: f64, (lo, hi): (f64, f64)| x < lo || x > hi;
            let who = format!("{} ({:.0}y, {})", run.at(s), run.age, b.label);
            if outside(s.r.hr, b.hr_possible) {
                bad.push((key("hr"), format!("{who} hr={:.0} — outside {:?} for this age", s.r.hr, b.hr_possible)));
            }
            if outside(s.r.rr, b.rr_possible) {
                bad.push((key("rr"), format!("{who} rr={:.0} — outside {:?} for this age", s.r.rr, b.rr_possible)));
            }
            if let Some(sbp) = s.r.sbp {
                if outside(sbp, b.sbp_possible) {
                    bad.push((key("sbp"), format!("{who} sbp={sbp:.0} — outside {:?} for this age", b.sbp_possible)));
                }
            }
            if let Some(spo2) = s.r.spo2 {
                if outside(spo2, SPO2_POSSIBLE) {
                    bad.push((key("spo2"), format!("{who} spo2={spo2:.0} — outside {SPO2_POSSIBLE:?}")));
                }
            }
            if outside(s.r.temp, TEMP_POSSIBLE) {
                bad.push((key("temp"), format!("{who} temp={:.1} — outside {TEMP_POSSIBLE:?}", s.r.temp)));
            }
        }
    }
    examined("readings against an age band", seen, 10_000);
    report("a reading was outside anything a body of that age produces", bad);
}

// ── 5. how deranged, as one comparable number ───────────────────────────────

/// One "point" is one clinically meaningful step outside the age-normal band.
///
/// This is an **ordering instrument and nothing else**. It is never printed, never scored, never
/// compared between patients — only between two moments of the *same* run of the *same* patient,
/// to answer questions of the form "did this get worse". NEWS2 would be the published answer and
/// cannot be used: it is not validated under sixteen, and five of these patients are children
/// (`vitals_web::news2`). So the bands are age-banded and the steps are stated here:
///
/// | axis | one point |
/// |------|-----------|
/// | hr   | 10 beats/min outside the age band |
/// | rr   | 4 breaths/min outside the age band |
/// | sbp  | 10 mmHg outside the age band |
/// | spo2 | 2 % below 94 |
/// | temp | 0.5 °C outside 36.1–38.0 |
/// | gcs  | 1 point below 15 |
///
/// Being inside the band scores nothing, at either end, so drifting from 99% to 95% saturation
/// is not "getting worse" here — which is deliberate. The instrument only speaks once a number
/// has left the range the bedside monitor treats as normal for that age.
fn outside(x: f64, (lo, hi): (f64, f64)) -> f64 {
    if x < lo {
        lo - x
    } else if x > hi {
        x - hi
    } else {
        0.0
    }
}

fn derangement(hr: f64, sbp: f64, spo2: f64, rr: f64, temp: f64, gcs: u8, age: f64) -> f64 {
    let b = band(age);
    outside(hr, b.hr) / 10.0
        + outside(sbp, b.sbp) / 10.0
        + outside(rr, b.rr) / 4.0
        + outside(spo2, SPO2_NORMAL) / 2.0
        + outside(temp, TEMP_NORMAL) / 0.5
        + f64::from(15u8.saturating_sub(gcs))
}

fn derangement_of(v: &Vitals, age: f64) -> f64 {
    derangement(v.hr, v.sbp, v.spo2, v.rr, v.temp, v.gcs, age)
}

/// The same number, broken out, for a failure message that says which axis.
fn breakdown(v: &Vitals, age: f64) -> String {
    let b = band(age);
    let mut parts = Vec::new();
    for (name, x, r) in [
        ("hr", v.hr, b.hr),
        ("sbp", v.sbp, b.sbp),
        ("rr", v.rr, b.rr),
        ("spo2", v.spo2, SPO2_NORMAL),
        ("temp", v.temp, TEMP_NORMAL),
    ] {
        if outside(x, r) > 0.0 {
            parts.push(format!("{name}={x:.1} (band {:.0}-{:.0})", r.0, r.1));
        }
    }
    if v.gcs < 15 {
        parts.push(format!("gcs={}", v.gcs));
    }
    if parts.is_empty() {
        "every axis inside its age band".into()
    } else {
        parts.join(" ")
    }
}

/// How bad a label claims things are. Only the ordering matters.
///
/// `Improving` sits above `Stable` on purpose: a patient the engine calls improving is one it
/// has already called worse, and every `improving` state on this shelf is entered from a
/// `deteriorating` or `critical` one. `Recovered` is the end of that road and sits at the bottom
/// with `Stable`.
fn rank(s: PatientStatus) -> u8 {
    match s {
        PatientStatus::Recovered => 0,
        PatientStatus::Stable => 1,
        PatientStatus::Improving => 2,
        PatientStatus::Deteriorating => 3,
        PatientStatus::Critical => 4,
        PatientStatus::Arrest => 5,
        PatientStatus::Dead => 6,
    }
}

/// Half a point — five beats, five millimetres of mercury, one percent of saturation.
///
/// A label is a coarse instrument and the boundary between two of them is a threshold crossing,
/// so the tick either side of a change is allowed to disagree by a hair. Anything larger is the
/// label and the numbers telling two different stories.
const STATUS_TOL: f64 = 0.5;

/// Escalating the label has to be earned by the numbers.
///
/// The reported failure was a screen where the words and the numbers disagreed, and this is the
/// general form of it: if the engine has called this patient `Critical`, then no moment of being
/// critical may look *better* than the best moment of being merely `Deteriorating` a minute
/// earlier. Compared within one run of one patient, so nothing here depends on a threshold
/// anybody has to defend — only on the case being internally consistent about which way is worse.
#[test]
fn a_worse_label_never_comes_with_better_numbers() {
    let mut bad = Vec::new();
    let mut seen = 0usize;
    for run in every_run() {
        let mut best: BTreeMap<u8, (PatientStatus, f64, u32)> = BTreeMap::new();
        for s in run.alive() {
            let d = derangement_of(&s.v, run.age);
            let e = best.entry(rank(s.status)).or_insert((s.status, d, s.t));
            if d < e.1 {
                *e = (s.status, d, s.t);
            }
        }
        for (lo_rank, (lo_st, lo_d, lo_t)) in &best {
            for (hi_rank, (hi_st, hi_d, hi_t)) in &best {
                if hi_rank <= lo_rank {
                    continue;
                }
                seen += 1;
                if *hi_d + STATUS_TOL < *lo_d {
                    bad.push((
                        format!("{}/{}/{lo_st:?}-{hi_st:?}", run.case, run.path.name()),
                        format!(
                            "{}/{}: {hi_st:?} at t={hi_t}s reads better than {lo_st:?} at t={lo_t}s \
                             (derangement {hi_d:.1} vs {lo_d:.1})",
                            run.case,
                            run.path.name()
                        ),
                    ));
                }
            }
        }
    }
    examined("ordered pairs of labels within one run", seen, 25);
    report("the label and the numbers told two different stories", bad);
}

// ── 6. direction ────────────────────────────────────────────────────────────

/// Did this axis move away from the patient's own age band, or back towards it?
///
/// Direction is not a property of the axis alone. A falling heart rate is recovery at 150 and
/// peri-arrest at 45; a falling temperature is defervescence at 39 and hypothermia at 35. So it
/// is read as distance from the band the bedside monitor treats as normal for this age, which
/// gets both of those right without anybody having to encode "tachycardia is bad".
///
/// `None` when the movement carries no direction: both ends inside the band, on an axis where
/// inside the band is simply normal, and 145 to 120 is a different pressure rather than a worse
/// one. Saturation and conscious level are the two exceptions — more is better at every value of
/// them — so a fall from 99% to 95% is still a fall, even though no monitor would alarm at either
/// end, and a `deteriorating` state driving the saturation *up* is still caught.
fn got_worse(axis: &str, x0: f64, x1: f64, age: f64) -> Option<bool> {
    let b = band(age);
    let normal = match axis {
        "hr" => b.hr,
        "rr" => b.rr,
        "sbp" => b.sbp,
        "spo2" => SPO2_NORMAL,
        "temp" => TEMP_NORMAL,
        "gcs" => (15.0, 15.0),
        _ => return None,
    };
    let (d0, d1) = (outside(x0, normal), outside(x1, normal));
    if d0 != d1 {
        return Some(d1 > d0);
    }
    match axis {
        "spo2" | "gcs" => Some(x1 < x0),
        _ => None,
    }
}

/// The smallest movement worth calling a movement, per axis. Below this it is arithmetic, not a
/// trend, and a suite that reports arithmetic gets switched off.
fn noise(axis: &str) -> f64 {
    match axis {
        "hr" => 2.0,
        "sbp" | "dbp" => 2.0,
        "spo2" => 1.0,
        "rr" => 1.0,
        "temp" => 0.2,
        _ => 0.5,
    }
}

/// A stretch of run during which the engine held one label.
struct Stretch {
    status: PatientStatus,
    from: usize,
    to: usize,
}

fn stretches(run: &Run) -> Vec<Stretch> {
    let mut out: Vec<Stretch> = Vec::new();
    for (i, s) in run.samples.iter().enumerate() {
        if s.ended {
            break;
        }
        match out.last_mut() {
            Some(last) if last.status == s.status => last.to = i,
            _ => out.push(Stretch { status: s.status, from: i, to: i }),
        }
    }
    out
}

/// Plausible is not the same as correct: a case can keep every number inside every bound and
/// still be driving the patient the wrong way.
///
/// So for each stretch of run the engine spent under one label, every vital that moved has to
/// have moved the way the label says. A `deteriorating` patient does not get better on an axis
/// the scenario is driving; an `improving` one does not get worse. A case that silently drove a
/// vital backwards would satisfy every bound in this file and fail here, and nothing else we run
/// would notice.
///
/// `Stable` is deliberately exempt. EP4's whole teaching is a pulmonary embolism that looks like
/// anxiety — a `stable` label over a saturation quietly falling from 92 to 86 — and a station
/// that could not do that could not teach the thing it exists to teach. What a `stable` label
/// does while the numbers move is reported by `the_frozen_variable_census`, not asserted here.
#[test]
fn a_case_moves_its_vitals_the_way_its_status_says_it_is_moving_them() {
    let mut bad = Vec::new();
    let mut judged = 0usize;
    for run in every_run() {
        for st in stretches(&run) {
            let seconds = run.samples[st.to].t - run.samples[st.from].t;
            if seconds < 30 {
                continue;
            }
            let want_worse = match st.status {
                PatientStatus::Deteriorating | PatientStatus::Critical | PatientStatus::Arrest => true,
                PatientStatus::Improving | PatientStatus::Recovered => false,
                PatientStatus::Stable | PatientStatus::Dead => continue,
            };
            let a = run.samples[st.from].v;
            let b = run.samples[st.to].v;
            for (axis, (x0, x1)) in VITALS.iter().zip(axes(&a).into_iter().zip(axes(&b))) {
                // The diastolic is derived from the systolic for every case on this shelf, so
                // checking it is checking the systolic twice.
                if *axis == "dbp" {
                    continue;
                }
                if (x1 - x0).abs() < noise(axis) {
                    continue;
                }
                let Some(worse) = got_worse(axis, x0, x1, run.age) else { continue };
                judged += 1;
                if worse != want_worse {
                    bad.push((
                        format!("{}/{}/{:?}/{axis}", run.case, run.path.name(), st.status),
                        format!(
                            "{}/{}: {:?} from t={}s to t={}s drove {axis} {x0:.1} → {x1:.1}, which is {} \
                             for a {:.0}-year-old — the label says the opposite",
                            run.case,
                            run.path.name(),
                            st.status,
                            run.samples[st.from].t,
                            run.samples[st.to].t,
                            if worse { "worse" } else { "better" },
                            run.age
                        ),
                    ));
                }
            }
        }
    }
    examined("(stretch, axis) movements with a direction to judge", judged, 40);
    report("a case drove a vital the opposite way from the one its status claims", bad);
}

// ── 7. arrest ───────────────────────────────────────────────────────────────

/// A patient in cardiac arrest has no pulse, and every panel on the screen has to agree.
///
/// `Rhythm` exists so shockability is a fact the engine holds rather than something a display
/// infers, and `reading.rs` keys the cuff and the oximeter off it: no output, no blood pressure,
/// no saturation, no respiratory rate. All of that is downstream of the *case* declaring a
/// rhythm on the state where the heart stops. A state that says `"status": "arrest"` and nothing
/// about the rhythm leaves the patient in whatever rhythm they were in — sinus — and the whole
/// chain reports a perfusing patient in cardiac arrest.
///
/// Checked twice: statically, so an author finds it, and on the walk, so nobody can argue about
/// whether the state is reachable.
#[test]
fn a_patient_in_arrest_has_no_pulse() {
    let mut bad = Vec::new();

    for case in CASES {
        let sce = load(case);
        for s in &sce.states {
            let declared = s.rhythm.as_deref().and_then(Rhythm::parse);
            if s.status.as_deref() == Some("arrest") && declared.is_none_or(|r| r.perfusing()) {
                bad.push((
                    format!("{case}/{}/static", s.id),
                    format!("{case}: state '{}' is an arrest and declares rhythm {:?} — the monitor \
                             will keep the last perfusing rhythm and print a pulse", s.id, s.rhythm),
                ));
            }
            // A state named after a rhythm and not declaring it is the same disagreement one step
            // earlier: `ep2`'s `vf` runs for ninety seconds with a pulse a defibrillator refuses.
            if let Some(named) = Rhythm::parse(&s.id) {
                if declared != Some(named) {
                    bad.push((
                        format!("{case}/{}/named", s.id),
                        format!("{case}: state '{}' is named for a rhythm and declares {:?}", s.id, s.rhythm),
                    ));
                }
            }
        }
    }

    for run in every_run() {
        for s in run.alive() {
            if s.status == PatientStatus::Arrest && s.r.pulse {
                bad.push((
                    format!("{}/{}/walked", run.case, run.path.name()),
                    format!(
                        "{} status=Arrest and the monitor reads hr={:.0} bp={:.0}/{:.0} spo2={:.0} rr={:.0} rhythm={}",
                        run.at(s),
                        s.r.hr,
                        s.r.sbp.unwrap_or(0.0),
                        s.r.dbp.unwrap_or(0.0),
                        s.r.spo2.unwrap_or(0.0),
                        s.r.rr,
                        s.r.rhythm
                    ),
                ));
            }
        }
    }
    report("a patient the engine says is in cardiac arrest still had a pulse", bad);
}

/// The same defect, from the end that costs a learner marks.
///
/// `defibrillate` charts a shock into a perfusing rhythm as harm — correctly, it is dangerous.
/// A candidate who defibrillates a patient the screen has just labelled `ARREST` should not be
/// the one charted for it.
#[test]
fn defibrillating_a_declared_arrest_is_not_charted_as_harm() {
    let mut bad = Vec::new();
    for case in CASES {
        let sce = load(case);
        let mut st = SceState::new(sce);
        let mut reached = None;
        for t in 1..=LIMIT_SEC {
            st.tick(1.0);
            if st.status == PatientStatus::Arrest && st.outcome().is_none() {
                reached = Some(t);
                break;
            }
            if st.outcome().is_some() {
                break;
            }
        }
        let Some(t) = reached else { continue };
        let r = st.defibrillate(200.0);
        if r == ShockResult::Perfusing {
            bad.push((
                case.to_string(),
                format!("{case} t={t}s: status=Arrest, and a 200 J shock is charted as harm to a perfusing patient"),
            ));
        }
    }
    report("a defibrillator offered a declared cardiac arrest was charted as harm", bad);
}

// ── 8. what a win leaves behind ─────────────────────────────────────────────

/// A case that ends in a win does not hand back a patient worse than the one it was given.
///
/// The direction check above reads one stretch at a time and can be satisfied by a run that
/// improves at the end after being driven somewhere it should never have gone. This reads the
/// whole arc: presentation against the last reading before the ending, on the same age-banded
/// instrument. It needs no threshold anybody has to defend — the comparison is the patient
/// against themselves.
#[test]
#[ignore = "DEFECT (4 runs, all on the `everything` path): osce-a wins at hr 133 having been \
            handed 118, osce-a2 at 139 from 124, osce-b2 at 118 from 105 (a fourteen-year-old, \
            band 60-100), and osce-d3 at 175 from 138 (a six-year-old, band 70-120). Every one is \
            the iatrogenic tachycardia from an IV-push adrenaline the case itself charts as harm: \
            the harm is recorded, the rubric marks it, and the physiology still hands out \
            `win_discharge` with the arrhythmia still running. Reported, not fixed — whether an \
            ending should be gated on the numbers is a design decision, not a bound."]
fn a_win_does_not_hand_back_a_worse_patient_than_it_was_given() {
    let mut bad = Vec::new();
    for run in every_run() {
        let Some(outcome) = run.outcome.as_deref() else { continue };
        if !outcome.starts_with("win") {
            continue;
        }
        let sce = load(run.case);
        let v0 = sce.vitals0;
        let d0 = derangement(v0.hr, v0.sbp, v0.spo2, v0.rr, v0.temp, v0.gcs, run.age);
        let Some(last) = run.samples.last() else { continue };
        let d1 = derangement_of(&last.v, run.age);
        if d1 > d0 + STATUS_TOL {
            bad.push((
                format!("{}/{}", run.case, run.path.name()),
                format!(
                    "{}/{}: {outcome} at t={}s leaves the patient worse than it found them — \
                     arrived {:.0}/{:.0} hr {:.0} spo2 {:.0} rr {:.0} (derangement {d0:.1}), \
                     ends {} (derangement {d1:.1})",
                    run.case,
                    run.path.name(),
                    last.t,
                    v0.sbp,
                    v0.dbp,
                    v0.hr,
                    v0.spo2,
                    v0.rr,
                    breakdown(&last.v, run.age),
                    ),
            ));
        }
    }
    report("a case awarded a win to a patient it had made worse", bad);
}

// ── 9. what never moves ─────────────────────────────────────────────────────

/// Which of the seven never move on this run, while the patient is alive.
fn frozen(run: &Run) -> Vec<&'static str> {
    let mut moved = [false; 7];
    let mut prev: Option<[f64; 7]> = None;
    for s in run.alive() {
        let now = axes(&s.v);
        if let Some(p) = prev {
            for (m, (a, b)) in moved.iter_mut().zip(p.into_iter().zip(now)) {
                if (a - b).abs() > 1e-9 {
                    *m = true;
                }
            }
        }
        prev = Some(now);
    }
    VITALS.iter().zip(moved).filter(|(_, m)| !m).map(|(n, _)| *n).collect()
}

/// The census, pinned: which vitals the clock never moves, on any of the three paths.
///
/// "The clock" is the qualifier that matters. Orders are given before the walk starts, so an
/// intervention's instantaneous `delta` is not counted as movement — what is being measured is
/// whether anything about this patient *evolves*, which is the question a monitor answers and the
/// question a frozen respiratory rate got wrong.
///
/// We found the frozen respiratory rate by accident, which is the part worth fixing. A variable
/// that never moves is not a passing test, it is a finding — a monitor with a number painted on
/// it — and the only way the next one announces itself is if the current list is written down
/// and a change to it fails.
///
/// A row here is not an accusation. `osce-b2`'s temperature moves and `osce-a`'s does not, and
/// there is nothing wrong with a station that does not teach fever. What is wrong is nobody
/// knowing which is which.
/// Three rows are worth reading twice, and all three are content questions rather than engine
/// ones — they are in the report for the clinical reviewer, not in an assertion:
///
///   * **`ep1` freezes `hr`.** A nineteen-year-old in anaphylactic shock holds 128 from the door
///     to the arrest, on all three paths. Every other axis moves.
///   * **`ep2` freezes `spo2` and `ep3` freezes `sbp`.** A STEMI that arrests with a saturation
///     of 96%, and a five-year-old who obstructs and dies at 96/58 the whole way down.
///   * **`osce-b` freezes both `sbp` and `spo2`.** The heart rate is the only visible number that
///     moves in the station; the deterioration lives entirely in a hidden `myocardium` axis.
///
/// And two cases stand completely still: `osce-b2` and `osce-c` untreated do not move a single
/// vital in half an hour of sim time, and `osce-c` treated does not move one either. Their
/// dynamics are all guarded behind flags only an order can set, so the patient in front of a
/// candidate who does nothing is a photograph. That is visible in the per-path table this test
/// prints and is not, by itself, an impossible reading.
const FROZEN: &[(&str, &str)] = &[
    ("ep1", "hr rr temp gcs"),
    ("ep2", "spo2 rr temp gcs"),
    ("ep3", "sbp dbp rr temp gcs"),
    ("ep4", "rr temp gcs"),
    ("ep5", "rr temp gcs"),
    ("osce-a", "rr temp gcs"),
    ("osce-a2", "rr temp gcs"),
    ("osce-b", "sbp dbp spo2 rr temp gcs"),
    ("osce-b2", "sbp dbp spo2 rr gcs"),
    ("osce-b3", "sbp dbp temp gcs"),
    ("osce-c", "sbp dbp rr temp gcs"),
    ("osce-c2", "sbp dbp rr temp gcs"),
    ("osce-c3", "sbp dbp rr gcs"),
    ("osce-d", "rr temp gcs"),
    ("osce-d2", "rr temp gcs"),
    ("osce-d3", "rr temp gcs"),
    ("osce-d4", "rr gcs"),
];

#[test]
fn the_frozen_variable_census_is_the_one_that_was_reviewed() {
    let runs = every_run();
    println!("\n── frozen per case/path (alive ticks only) ──");
    let mut union: BTreeMap<&str, Option<Vec<&str>>> = BTreeMap::new();
    for run in &runs {
        let f = frozen(run);
        println!(
            "  {:8} {:10} {:>5}s  outcome={:14} frozen: {}",
            run.case,
            run.path.name(),
            run.samples.last().map(|s| s.t).unwrap_or(0),
            run.outcome.as_deref().unwrap_or(if run.open { "(still running)" } else { "-" }),
            if f.is_empty() { "-".to_string() } else { f.join(" ") }
        );
        match union.entry(run.case).or_default() {
            Some(seen) => seen.retain(|x| f.contains(x)),
            slot => *slot = Some(f),
        }
    }

    println!("\n── frozen on every path (the pinned census) ──");
    let mut got: Vec<(String, String)> = Vec::new();
    for case in CASES {
        let f = union.remove(case).flatten().unwrap_or_default();
        let line = f.join(" ");
        println!("    (\"{case}\", \"{line}\"),");
        got.push((case.to_string(), line));
    }

    let want: BTreeMap<&str, &str> = FROZEN.iter().copied().collect();
    let mut moved: Vec<String> = Vec::new();
    for (case, line) in &got {
        match want.get(case.as_str()) {
            Some(w) if *w == line => {}
            Some(w) => moved.push(format!("  {case}\n    reviewed: {w}\n    now:      {line}")),
            None => moved.push(format!("  {case} is not in the census\n    now:      {line}")),
        }
    }
    assert!(
        moved.is_empty(),
        "{} case(s) froze or thawed a vital sign since the census was reviewed. A thaw is good \
         news and still has to be looked at; a freeze is a monitor with a number painted on it. \
         The table above is the new census, ready to paste.\n{}",
        moved.len(),
        moved.join("\n")
    );
}

/// A variable that never moves in *any* case is not a scenario choice, it is a dead axis.
#[test]
#[ignore = "DEFECT: `gcs` never moves in any of the seventeen cases while the patient is alive. \
            Every case declares a conscious level in `vitals0` and no case ever changes it — not \
            the anaphylaxis that closes an airway, not the haemorrhage, not the two children who \
            obstruct. It moves exactly once, to 3, in `terminate`. A consciousness that only \
            changes at death cannot teach the deterioration it is the earliest sign of."]
fn no_vital_sign_is_frozen_in_every_case_on_the_shelf() {
    let runs = every_run();
    let mut dead_axes = Vec::new();
    for axis in VITALS {
        if axis == "dbp" {
            continue; // derived from the systolic; it moves exactly when that does.
        }
        let cases: Vec<&str> = CASES
            .iter()
            .copied()
            .filter(|c| runs.iter().filter(|r| r.case == *c).all(|r| frozen(r).contains(&axis)))
            .collect();
        if cases.len() == CASES.len() {
            dead_axes.push((
                axis.to_string(),
                format!("{axis} never moves in any of the {} cases on the shelf", CASES.len()),
            ));
        }
    }
    report("a vital sign is painted on rather than simulated", dead_axes);
}

// ── 10. the scale that has no bottom ────────────────────────────────────────

/// The Glasgow Coma Scale runs 3 to 15. There is no 2, no 1 and no 0.
///
/// `SceState::set_var` clamps `gcs` to `0..=15`, so an authored `{"set": {"gcs": 0}}` produces a
/// score the scale does not contain — and it would print, and score, and be believed. No case on
/// the shelf writes `gcs` today, which is exactly why this is worth pinning now: the clamp is
/// wrong and nothing reaches it, so nothing will find it except the day somebody authors an
/// unresponsive patient.
#[test]
#[ignore = "DEFECT (latent): `SceState::set_var` clamps gcs to 0..=15. The Glasgow Coma Scale has \
            no score below 3, so an authored `set` of 0, 1 or 2 yields an impossible score. \
            Unreachable from the seventeen shipped cases — none of them writes gcs at all."]
fn the_glasgow_coma_scale_has_no_score_below_three() {
    let sce = Sce::from_json(
        r#"{
          "vitals0": { "hr": 80, "sbp": 120, "dbp": 80, "spo2": 98, "rr": 14, "temp": 37.0, "gcs": 15 },
          "initial_state": "obtunded",
          "states": [
            { "id": "obtunded", "status": "critical",
              "transitions": [ { "to_state": "obtunded",
                                 "do": [ { "set": { "gcs": 0 } } ],
                                 "when": { "var": "t_elapsed", "op": "ge", "value": 1 } } ] }
          ],
          "outcomes": [ { "id": "win_icu", "kind": "win" } ]
        }"#,
    )
    .expect("fixture parses");
    let mut st = SceState::new(sce);
    st.tick(1.0);
    assert!(
        st.vitals.gcs >= 3,
        "an authored `set` of gcs 0 reached the monitor as {} — the scale starts at 3",
        st.vitals.gcs
    );
}
