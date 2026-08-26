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
