//! Deterministic OSCE rubric scorer — the re-derivable 40.
//!
//! A rubric is a list of checks against a *replayed* run's event log (`SceState::events()`), which
//! `vitals-replay::resume` reproduces byte-for-byte from the tape. Nothing here reads a language
//! model or a keyword matcher over free text the player typed — it reads what the deterministic
//! automaton *did*: the actions it recorded, the harms it fired, the terminal outcome it reached.
//! That is the whole reason this score, and only this score, may drive an on-chain claim
//! (`docs/RISKS.md` §3): a stranger re-runs the engine and gets the same number.
//!
//! The rubric itself is **data**, authored per case (JSON), so the clinical content — which action
//! is critical, what the time window is, which harm must be avoided — lives outside this engine and
//! is reviewed by clinicians, not compiled in. `status` lets a provisional rubric say so.
#![forbid(unsafe_code)]

use serde::Deserialize;
use vitals_replay::{resume, Step};
use vitals_sce::runtime::Event;
use vitals_sce::text::canon;

/// One deterministic check against the event log. Internally tagged by `type` so a rubric reads
/// as flat JSON: `{"label":"...","type":"action","needle":"oxygen","points":10}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Check {
    /// An `action` event whose text contains `needle` was recorded.
    Action { needle: String, points: u16 },
    /// The same, and no later than `by_sec` on the scenario clock — timing is part of competence.
    ActionBy { needle: String, by_sec: f64, points: u16 },
    /// An `action` matching *any* needle happened — for a step with clinically equivalent routes
    /// (reperfusion by PCI or by thrombolysis), so taking the right one is not penalised.
    ActionAny { any_of: Vec<String>, points: u16 },
    /// No `harm` event whose text contains `needle` occurred. Points are earned by *avoiding* it.
    NoHarm { needle: String, points: u16 },
    /// The run reached a terminal `outcome` named in `any_of`.
    Outcome { any_of: Vec<String>, points: u16 },
}

impl Check {
    pub fn points(&self) -> u16 {
        match self {
            Check::Action { points, .. }
            | Check::ActionBy { points, .. }
            | Check::ActionAny { points, .. }
            | Check::NoHarm { points, .. }
            | Check::Outcome { points, .. } => *points,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Item {
    pub label: String,
    #[serde(flatten)]
    pub check: Check,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rubric {
    /// Which case this rubric marks. Bound into the record's `rubric_hash` upstream so a re-mark
    /// and a re-word are distinguishable.
    pub case: String,
    /// Basis points of the deterministic score that earn a star. 7000 = 70%.
    #[serde(default = "default_pass")]
    pub pass_bps: u32,
    /// Free-text provenance / review state. A provisional rubric announces itself here.
    #[serde(default)]
    pub status: String,
    pub items: Vec<Item>,
}
fn default_pass() -> u32 {
    7000
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemResult {
    pub label: String,
    pub points: u16,
    pub earned: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetResult {
    pub earned: u16,
    pub max: u16,
    pub items: Vec<ItemResult>,
}

impl DetResult {
    /// The deterministic score in basis points, the shape `vitals-progress::dreyfus` consumes.
    /// Zero max scores as zero — an empty rubric proves nothing, it does not prove perfection.
    pub fn bps(&self) -> u32 {
        if self.max == 0 {
            0
        } else {
            (self.earned as u32 * 10_000) / self.max as u32
        }
    }
    /// Whether this run clears the rubric's bar — earns a star.
    pub fn cleared(&self, rubric: &Rubric) -> bool {
        self.max > 0 && self.bps() >= rubric.pass_bps
    }
}

/// The scenario clock of the first `kind` event whose text contains `needle`, canonicalised on
/// both sides so a full-width or NFC/NFD variant cannot slip a match. `None` if it never happened.
fn hit(events: &[Event], kind: &str, needle: &str) -> Option<f64> {
    let n = canon(needle).to_lowercase();
    events
        .iter()
        .filter(|e| e.kind == kind)
        .find(|e| canon(&e.text).to_lowercase().contains(&n))
        .map(|e| e.t_sec)
}

/// Score a replayed run against a rubric. Pure and total: same events + same rubric → same result,
/// which is the property the claim path depends on.
pub fn score(events: &[Event], rubric: &Rubric) -> DetResult {
    let mut earned = 0u16;
    let mut max = 0u16;
    let mut items = Vec::with_capacity(rubric.items.len());
    for it in &rubric.items {
        let p = it.check.points();
        max = max.saturating_add(p);
        let ok = match &it.check {
            Check::Action { needle, .. } => hit(events, "action", needle).is_some(),
            Check::ActionBy { needle, by_sec, .. } => {
                hit(events, "action", needle).is_some_and(|t| t <= *by_sec)
            }
            Check::ActionAny { any_of, .. } => {
                any_of.iter().any(|n| hit(events, "action", n).is_some())
            }
            Check::NoHarm { needle, .. } => hit(events, "harm", needle).is_none(),
            Check::Outcome { any_of, .. } => {
                any_of.iter().any(|o| hit(events, "outcome", o).is_some())
            }
        };
        if ok {
            earned = earned.saturating_add(p);
        }
        items.push(ItemResult { label: it.label.clone(), points: p, earned: ok });
    }
    DetResult { earned, max, items }
}

/// Score a finished run for anchoring, in one call: replay its tape, mark it against `rubric_json`,
/// and return `(det_score, det_max, rubric_hash)` ready to stamp into the leaf.
///
/// This is the exact path a verifier re-runs — `vitals_replay::resume` reproduces the events from
/// the tape, `score` marks them, `rubric_hash` pins which rubric did the marking — so `det_score`
/// is re-derivable by anyone with the tape and the pinned rubric, which is what lets it, and only
/// it, back an on-chain claim. The rubric is passed as its authored bytes so the hash a verifier
/// recomputes is byte-identical.
pub fn det_for_run(
    sce_json: &str,
    tape: &[Step],
    rubric_json: &str,
) -> Result<(u16, u16, [u8; 32]), String> {
    let rubric: Rubric = serde_json::from_str(rubric_json).map_err(|e| e.to_string())?;
    let (state, _replay) = resume(sce_json, tape)?;
    let det = score(state.events(), &rubric);
    Ok((det.earned, det.max, vitals_progress::record::rubric_hash(rubric_json.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, text: &str, t: f64) -> Event {
        Event { t_sec: t, kind: kind.into(), text: text.into() }
    }
    fn rubric() -> Rubric {
        serde_json::from_str(
            r#"{ "case":"test", "pass_bps":7000, "items":[
                {"label":"oxygen","type":"action","needle":"oxygen","points":10},
                {"label":"early abx","type":"action_by","needle":"antibiotic","by_sec":600,"points":10},
                {"label":"no wrong shock","type":"no_harm","needle":"shock","points":10},
                {"label":"survives","type":"outcome","any_of":["WinDischarge","WinIcu"],"points":10}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn perfect_run_scores_full_and_clears() {
        let evs = [
            ev("action", "gave Oxygen face mask", 30.0),
            ev("action", "antibiotic ceftriaxone", 300.0),
            ev("outcome", "WinDischarge", 900.0),
        ];
        let r = score(&evs, &rubric());
        assert_eq!((r.earned, r.max), (40, 40));
        assert_eq!(r.bps(), 10_000);
        assert!(r.cleared(&rubric()));
    }

    #[test]
    fn a_late_critical_action_loses_only_its_points() {
        let evs = [
            ev("action", "oxygen", 10.0),
            ev("action", "antibiotic", 900.0), // past the 600s window
            ev("outcome", "WinDischarge", 1000.0),
        ];
        let r = score(&evs, &rubric());
        assert_eq!(r.earned, 30); // oxygen + no-harm(shock absent) + outcome; abx too late
    }

    #[test]
    fn a_harm_event_fails_the_avoidance_check() {
        let evs = [
            ev("harm", "shock on a perfusing rhythm", 50.0),
            ev("action", "oxygen", 10.0),
        ];
        let r = score(&evs, &rubric());
        assert_eq!(r.earned, 10); // only oxygen; shock harm fired, abx & outcome absent
        assert!(!r.cleared(&rubric()));
    }

    #[test]
    fn canon_defeats_a_fullwidth_dodge() {
        // NFKC folds full-width ＯＸＹＧＥＮ to ASCII, so a cosmetic variant still matches.
        let evs = [ev("action", "ＯＸＹＧＥＮ mask", 5.0)];
        assert!(hit(&evs, "action", "oxygen").is_some());
    }

    #[test]
    fn action_any_credits_either_equivalent_route() {
        let r: Rubric = serde_json::from_str(
            r#"{"case":"t","items":[
                {"label":"reperfusion","type":"action_any","any_of":["cath","thrombolys"],"points":12}
            ]}"#,
        )
        .unwrap();
        // PCI alone earns it; thrombolysis alone earns it; neither earns nothing.
        assert_eq!(score(&[ev("action", "cath_lab", 400.0)], &r).earned, 12);
        assert_eq!(score(&[ev("action", "thrombolysis", 400.0)], &r).earned, 12);
        assert_eq!(score(&[ev("action", "aspirin", 60.0)], &r).earned, 0);
    }

    #[test]
    fn empty_rubric_proves_nothing() {
        let r = DetResult { earned: 0, max: 0, items: vec![] };
        assert_eq!(r.bps(), 0);
        assert!(!r.cleared(&rubric()));
    }

    #[test]
    fn det_for_run_marks_a_real_replay_and_pins_the_rubric() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}scenarios/ep2-stemi.json")).unwrap();
        let rubric = std::fs::read_to_string(format!("{root}rubrics/ep2-stemi.json")).unwrap();
        // A competent STEMI: ECG, aspirin, oxygen, reperfuse, then time to salvage muscle.
        let tape = vec![
            Step::Do("12-lead ecg".into()),
            Step::Tick(20.0),
            Step::Do("aspirin 300 chewed".into()),
            Step::Tick(15.0),
            Step::Do("oxygen".into()),
            Step::Tick(15.0),
            Step::Do("activate the cath lab for pci".into()),
            Step::Tick(200.0),
            Step::Tick(60.0),
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric).unwrap();
        assert_eq!((s, m), (40, 40)); // full marks — the re-derivable 40
        assert_ne!(rhash, [0u8; 32]);
        // The pin is stable and equals the standalone hash of the same bytes.
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric.as_bytes()));
    }

    /// Station A (osce-a, from embla-cases ddx-anaphylaxis-1), marked by the exact anchor path.
    /// The competent tape speaks in the chip texts the UI actually sends — asks included, because
    /// at a station an ask IS a do — and earns the full 40; the antihistamine-and-wait tape lets
    /// the adrenaline window close and lands under the bar, which is what makes the star a claim.
    #[test]
    fn station_a_competent_run_clears_and_hesitation_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-a.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-a.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-a: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("any allergies?".into()),
            Step::Tick(15.0),
            Step::Do("what did you eat before this?".into()),
            Step::Tick(15.0),
            Step::Do("adrenaline im".into()), // t=30 — inside the five-minute window
            Step::Tick(10.0),
            Step::Do("oxygen mask".into()),
            Step::Tick(10.0),
            Step::Do("serum tryptase".into()),
            Step::Tick(5.0),
            Step::Do("12-lead ecg".into()),
            Step::Tick(5.0),
            Step::Do("anaphylaxis".into()),
            Step::Tick(160.0), // the observation window runs out to discharge
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // Antihistamine alone, then waiting: the delay harm fires at five minutes, the patient
        // arrests, and the run must not clear — retry with feedback is the mastery model.
        let hesitation = vec![
            Step::Do("chlorpheniramine".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &hesitation, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![] };
        assert!(!det.cleared(&r), "a run without adrenaline must not clear: {s}/{m}");
    }

    /// Station B (osce-b, from embla-cases ddx-possible-nstemi-stemi-2): chest pain → ECG inside
    /// ten minutes → reperfusion decision. The cath lab refuses a hunch — activating it without
    /// an ECG earns the action but never the outcome, and stays under the bar.
    #[test]
    fn station_b_competent_run_clears_and_a_hunch_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-b.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-b.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-b: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("where is the pain?".into()),
            Step::Tick(20.0),
            Step::Do("any risk factors — smoking, sugar, pressure?".into()),
            Step::Tick(20.0),
            Step::Do("12-lead ecg".into()), // t=40 — door-to-ECG well inside ten minutes
            Step::Tick(10.0),
            Step::Do("aspirin 300 chewed".into()),
            Step::Tick(10.0),
            Step::Do("troponin".into()),
            Step::Tick(10.0),
            Step::Do("acute stemi".into()),
            Step::Tick(5.0),
            Step::Do("activate the cath lab".into()),
            Step::Tick(190.0), // door-to-balloon runs; muscle mostly intact → win_discharge
        ];
        let (s, m, _) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        // Cath lab on a hunch, no ECG ever: the reperfusion flag never sets, the ten-minute harm
        // fires, the infarct completes. The action_any credit alone cannot reach the bar.
        let hunch = vec![
            Step::Do("activate the cath lab".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &hunch, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![] };
        assert!(!det.cleared(&r), "reperfusion credit without the ECG must not clear: {s}/{m}");
    }

    /// Station C (osce-c, from embla-cases ddx-croup-2): a drooling child who looks like EP3's
    /// boy but is loud, afebrile, vaccinated and barking — croup, not epiglottitis. The competent
    /// tape scores the severity from the doorway, gives the steroid, and touches nothing that
    /// does not need touching. The tongue-depressor tape does everything else right — including
    /// the nebulised-adrenaline rescue — and still lands under the bar, because the one
    /// invasive exam is the station's whole lesson.
    #[test]
    fn station_c_competent_run_clears_and_the_tongue_depressor_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-c.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-c.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-c: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("has she had this before?".into()),
            Step::Tick(10.0),
            Step::Do("any fever?".into()),
            Step::Tick(10.0),
            Step::Do("are her shots up to date?".into()),
            Step::Tick(10.0),
            Step::Do("score her from the doorway".into()),
            Step::Tick(10.0),
            Step::Do("check the saturations".into()),
            Step::Tick(10.0),
            Step::Do("neck and chest films".into()),
            Step::Tick(10.0),
            Step::Do("dexamethasone syrup".into()), // t=60 — the whole visit, given early
            Step::Tick(10.0),
            Step::Do("keep her on mum's lap".into()),
            Step::Tick(10.0),
            Step::Do("watch her for two hours".into()),
            Step::Tick(10.0),
            Step::Do("croup".into()),
            Step::Tick(160.0), // the observation runs out to a discharge before midnight
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // The depressor goes in early; the child cries herself into stridor; the rescue and
        // every other mark are earned — and the run must still fail on the harm it caused.
        let depressor = vec![
            Step::Do("any fever?".into()),
            Step::Tick(5.0),
            Step::Do("score her from the doorway".into()),
            Step::Tick(5.0),
            Step::Do("look in the throat".into()), // the one forbidden exam
            Step::Tick(5.0),
            Step::Do("nebulised adrenaline".into()), // competent rescue, too late to matter
            Step::Tick(5.0),
            Step::Do("has she had this before?".into()),
            Step::Tick(5.0),
            Step::Do("are her shots up to date?".into()),
            Step::Tick(5.0),
            Step::Do("check the saturations".into()),
            Step::Tick(5.0),
            Step::Do("neck and chest films".into()),
            Step::Tick(5.0),
            Step::Do("dexamethasone syrup".into()),
            Step::Tick(5.0),
            Step::Do("watch her for two hours".into()),
            Step::Tick(5.0),
            Step::Do("croup".into()),
            Step::Tick(130.0), // she settles — into an admission, not a discharge
        ];
        let (s, m, _) = det_for_run(&sce, &depressor, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![] };
        assert!(!det.cleared(&r), "the tongue depressor must not clear the bar: {s}/{m}");
    }

    /// Station D (osce-d, from embla-cases embla-upper-gastrointestinal-bleeding-intern): call
    /// the shock while he compensates, two big lines, crystalloid AND blood, then the scope.
    /// GI is gated like B's cath lab — no resuscitation, no endoscopy. The failing tape is the
    /// chest-pain reflex: aspirin for a CAD history, one bag of fluid through a thin line, hope.
    #[test]
    fn station_d_competent_run_clears_and_the_chest_pain_reflex_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-d.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-d.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-d: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("what pills do you take every day?".into()),
            Step::Tick(10.0),
            Step::Do("how much blood — what colour?".into()),
            Step::Tick(10.0),
            Step::Do("feel his hands, look at his eyes".into()),
            Step::Tick(10.0),
            Step::Do("two large-bore lines".into()), // t=30 — access before the fourth minute
            Step::Tick(10.0),
            Step::Do("group and crossmatch four units".into()),
            Step::Tick(10.0),
            Step::Do("warmed crystalloid, wide open".into()),
            Step::Tick(10.0),
            Step::Do("transfuse packed cells".into()),
            Step::Tick(10.0),
            Step::Do("pantoprazole bolus and infusion".into()),
            Step::Tick(10.0),
            Step::Do("hold the aspirin and clopidogrel".into()),
            Step::Tick(10.0),
            Step::Do("rectal exam".into()),
            Step::Tick(10.0),
            Step::Do("upper gi bleed".into()),
            Step::Tick(10.0),
            Step::Do("call gi — urgent endoscopy".into()), // tank full — GI takes the call
            Step::Tick(160.0), // clipped, dry, and up to the unit
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // Aspirin for the stent, one bag through a thin line, and waiting: the access harm
        // fires at four minutes, the re-bleed at five finds an empty tank, and he exsanguinates.
        let reflex = vec![
            Step::Do("aspirin 300 chewed".into()),
            Step::Tick(20.0),
            Step::Do("warmed crystalloid, wide open".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &reflex, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![] };
        assert!(!det.cleared(&r), "the chest-pain reflex must not clear the bar: {s}/{m}");
    }

    /// Station A2 (osce-a2, from embla-cases ddx-anaphylaxis-2): anaphylaxis in a gut disguise —
    /// cramps, diarrhoea, a blackout she calls sitting down, and a pressure already at 86. The
    /// competent tape digs the collapse out of an evasive history and treats the shock, not the
    /// itch. The failing tape is the antihistamine-first reflex: chlorpheniramine, then waiting,
    /// while both the reflex harm and the five-minute window fire and the pressure runs out.
    #[test]
    fn station_a2_competent_run_clears_and_the_antihistamine_reflex_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-a2.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-a2.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-a2: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("any allergies?".into()),
            Step::Tick(10.0),
            Step::Do("what did you eat today?".into()),
            Step::Tick(10.0),
            Step::Do("did you faint — even for a moment?".into()),
            Step::Tick(10.0),
            Step::Do("adrenaline im".into()), // t=30 — well inside the five-minute window
            Step::Tick(10.0),
            Step::Do("oxygen mask".into()),
            Step::Tick(5.0),
            Step::Do("normal saline bolus".into()),
            Step::Tick(5.0),
            Step::Do("serum tryptase".into()),
            Step::Tick(5.0),
            Step::Do("anaphylaxis".into()),
            Step::Tick(160.0), // the observation window runs out to discharge
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // Chlorpheniramine for the itch, then waiting: the antihistamine-first harm fires at two
        // minutes, the window harm at five, and she arrests — nowhere near the bar.
        let reflex = vec![
            Step::Do("chlorpheniramine".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &reflex, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![] };
        assert!(!det.cleared(&r), "the antihistamine reflex must not clear the bar: {s}/{m}");
    }

    /// Station B2 (osce-b2, from embla-cases ddx-pericarditis-1): ST elevation is a finding, not
    /// a diagnosis. A febrile fourteen-year-old with a rub and a favourite chair — the competent
    /// tape reads the shape off twelve leads and treats with tablets. The failing tape is the
    /// STEMI reflex from station B run on the wrong chest: aspirin, cath lab, lytics — and the
    /// sac fills with blood.
    #[test]
    fn station_b2_competent_run_clears_and_the_stemi_reflex_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-b2.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-b2.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-b2: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("where is the pain — what makes it better?".into()),
            Step::Tick(10.0),
            Step::Do("does breathing change it?".into()),
            Step::Tick(10.0),
            Step::Do("any fever or a cold lately?".into()),
            Step::Tick(10.0),
            Step::Do("listen to the heart — sit him forward".into()),
            Step::Tick(10.0),
            Step::Do("12-lead ecg".into()), // t=40 — inside ten minutes, and read for its shape
            Step::Tick(10.0),
            Step::Do("troponin".into()),
            Step::Tick(10.0),
            Step::Do("echocardiogram".into()),
            Step::Tick(10.0),
            Step::Do("pericarditis".into()),
            Step::Tick(5.0),
            Step::Do("ibuprofen with food".into()),
            Step::Tick(160.0), // rest on tablets runs out to a discharge
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // The reflex: call it a STEMI, load aspirin, spin the lab, push the lytic. The sac
        // fills, the pressure falls, and the run lands nowhere near the bar.
        let reflex = vec![
            Step::Do("12-lead ecg".into()),
            Step::Tick(10.0),
            Step::Do("aspirin 300 chewed".into()),
            Step::Tick(10.0),
            Step::Do("call it a stemi".into()),
            Step::Tick(10.0),
            Step::Do("activate the cath lab".into()),
            Step::Tick(10.0),
            Step::Do("thrombolysis".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &reflex, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![] };
        assert!(!det.cleared(&r), "the stemi reflex must not clear the bar: {s}/{m}");
    }

    /// Station B3 (osce-b3, from embla-cases ddx-croup-1): the mild rung of the croup ladder —
    /// grade her from the doorway, one syrup of dexamethasone, an hour of watching, the speech,
    /// home. The failing tape treats a virus with antibiotics and reaches for the door with
    /// nothing behind it; the steroid never comes, and the airway stops waiting.
    #[test]
    fn station_b3_competent_run_clears_and_antibiotics_for_a_virus_fail() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-b3.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-b3.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-b3: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("when did the bark start?".into()),
            Step::Tick(10.0),
            Step::Do("any fever?".into()),
            Step::Tick(10.0),
            Step::Do("is she drinking?".into()),
            Step::Tick(10.0),
            Step::Do("score her from the doorway".into()),
            Step::Tick(10.0),
            Step::Do("check the saturations".into()),
            Step::Tick(10.0),
            Step::Do("neck and chest films".into()),
            Step::Tick(10.0),
            Step::Do("dexamethasone syrup".into()), // t=60 — the whole visit, given early
            Step::Tick(10.0),
            Step::Do("watch her for an hour".into()),
            Step::Tick(10.0),
            Step::Do("give the safety-net advice".into()),
            Step::Tick(10.0),
            Step::Do("croup".into()),
            Step::Tick(220.0), // the watched hour runs out to a discharge
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // Amoxicillin for a bark and straight for the door: the discharge harm fires, the
        // steroid never comes, the seventh minute tips her into the night she didn't have.
        let bottle = vec![
            Step::Do("any fever?".into()),
            Step::Tick(10.0),
            Step::Do("amoxicillin for the throat".into()),
            Step::Tick(10.0),
            Step::Do("send her home".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &bottle, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![] };
        assert!(!det.cleared(&r), "antibiotics-and-home must not clear the bar: {s}/{m}");
    }

    /// Station C2 (osce-c2, from embla-cases ddx-bronchospasm-acute-asthma-exacerbation-2): the
    /// bronchodilator ladder with its feet on a number — peak flow, salbutamol, ipratropium,
    /// peak flow again, and a systemic steroid behind the neb. The failing tape reads her fear
    /// as panic and sedates the only drive that was holding her.
    #[test]
    fn station_c2_competent_run_clears_and_the_sedative_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-c2.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-c2.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-c2: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("how often does this happen?".into()),
            Step::Tick(10.0),
            Step::Do("can you finish a sentence?".into()),
            Step::Tick(10.0),
            Step::Do("listen to the chest".into()),
            Step::Tick(10.0),
            Step::Do("peak flow — measure it".into()), // the baseline number
            Step::Tick(10.0),
            Step::Do("oxygen".into()),
            Step::Tick(10.0),
            Step::Do("salbutamol neb".into()), // t=50 — the first rung
            Step::Tick(10.0),
            Step::Do("prednisolone 40 mg".into()), // t=60 — the steroid behind the neb, early
            Step::Tick(10.0),
            Step::Do("ipratropium".into()), // the second rung
            Step::Tick(10.0),
            Step::Do("peak flow again".into()), // reassess — the ladder answers in numbers
            Step::Tick(10.0),
            Step::Do("inhaled steroid + action plan".into()),
            Step::Tick(5.0),
            Step::Do("acute asthma exacerbation".into()),
            Step::Tick(250.0), // settled, rechecked, steroid on board → home with a plan
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // One neb, then diazepam for the "panic", twice more when she keeps fighting it: the
        // respiratory drive goes, the saturation follows, and the run must not clear.
        let sedated = vec![
            Step::Do("salbutamol neb".into()),
            Step::Tick(30.0),
            Step::Do("diazepam for the panic".into()),
            Step::Tick(30.0),
            Step::Do("more diazepam".into()),
            Step::Tick(30.0),
            Step::Do("more diazepam".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &sedated, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![] };
        assert!(!det.cleared(&r), "the sedative must not clear the bar: {s}/{m}");
    }

    /// Station C3 (osce-c3, from embla-cases ddx-pneumonia-2): the diagnosis is the easy half —
    /// the station is disposition. CURB-65 of zero on a woman saturating 93%: score her, dose
    /// her inside the hour, and let the oximeter outvote the arithmetic. The failing tape trusts
    /// the score and sends the lobe home untreated.
    #[test]
    fn station_c3_competent_run_clears_and_the_score_says_home_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-c3.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-c3.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-c3: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("tell me about the cough".into()),
            Step::Tick(10.0),
            Step::Do("any illnesses? do you smoke?".into()),
            Step::Tick(10.0),
            Step::Do("listen to the chest".into()),
            Step::Tick(10.0),
            Step::Do("count the breathing".into()),
            Step::Tick(10.0),
            Step::Do("chest x-ray".into()),
            Step::Tick(10.0),
            Step::Do("full blood count".into()),
            Step::Tick(10.0),
            Step::Do("sputum and blood cultures".into()),
            Step::Tick(10.0),
            Step::Do("curb-65 — score her".into()),
            Step::Tick(10.0),
            Step::Do("co-amoxiclav plus macrolide — first dose now".into()), // t=80, inside the hour
            Step::Tick(10.0),
            Step::Do("oxygen".into()),
            Step::Tick(10.0),
            Step::Do("pneumonia".into()),
            Step::Tick(5.0),
            Step::Do("admit to a short-stay bed".into()),
            Step::Tick(160.0), // the ward takes her, first dose already running
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // "She scored zero": the score is right, the disposition is wrong. Home untreated, the
        // hour slides past, the lobe keeps growing — the harm and the missing dose sink it.
        let arithmetic = vec![
            Step::Do("curb-65 — score her".into()),
            Step::Tick(10.0),
            Step::Do("pneumonia".into()),
            Step::Tick(10.0),
            Step::Do("home with tablets".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &arithmetic, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![] };
        assert!(!det.cleared(&r), "score-says-home must not clear the bar: {s}/{m}");
    }

    #[test]
    fn every_rubric_uses_the_canonical_star_bar() {
        // Enforce-equal: a rubric whose pass_bps drifts from the one global bar is a failing test,
        // not a silent bug — so the star a verifier re-derives from the pinned rubric and the star
        // the tally counts against the global cannot disagree.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/rubrics");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let r: Rubric = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(
                r.pass_bps,
                vitals_progress::STAR_PASS_BPS,
                "{:?} pass_bps drifted from the canonical star bar",
                path.file_name().unwrap()
            );
            checked += 1;
        }
        assert!(checked >= 3, "expected the three seeded rubrics at least, found {checked}");
    }
}
