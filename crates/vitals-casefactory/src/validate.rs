//! The gate a pack passes before it is written: parsed by the engine, replayed to a death
//! untreated and to a win along its own path, marked by the scorer, every needle resolving, no
//! season marker and no patient name anywhere in it.

use crate::archetype::Archetype;
use crate::text::{name_tokens, strings};
use serde::Serialize;
use serde_json::Value;
use vitals_sce::{Sce, SceState};

/// One order on the recorded management path.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PathStep {
    pub t_sec: f64,
    pub id: String,
}

/// What the replays proved about this pack.
#[derive(Debug, Clone, Serialize)]
pub struct Proof {
    /// Sim seconds from the first tick to the death outcome with nobody acting.
    pub untreated_death_sec: f64,
    /// The orders, in order and on the clock, that reach the win.
    pub win_path: Vec<PathStep>,
    /// Sim seconds from the first tick to the win outcome along `win_path`.
    pub win_sec: f64,
    /// The win outcome id reached.
    pub win_outcome: String,
    /// What the derived rubric gave the path: it has to clear its own bar.
    pub golden_score: GoldenScore,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoldenScore {
    pub earned: u16,
    pub max: u16,
    pub pass_bps: u32,
}

/// Strings that belong to the season's content and must not appear in a World pack.
pub const SEASON_MARKERS: &[&str] = &["osce-", "Somsri", "Somchai", "station ", "/img/", "/clip/"];

fn has_episode_marker(s: &str) -> bool {
    let b = s.as_bytes();
    for i in 0..b.len().saturating_sub(2) {
        if &b[i..i + 2] == b"EP" && (b'1'..=b'5').contains(&b[i + 2]) {
            let before_ok = i == 0 || !b[i - 1].is_ascii_alphanumeric();
            let after_ok = i + 3 >= b.len() || !b[i + 3].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
        }
    }
    false
}

/// Anything in the pack that names the season or the patient. Empty means clean.
pub fn scan(pack: &Value, patient_name: &str) -> Vec<String> {
    let mut all = Vec::new();
    strings(pack, "$", &mut all);
    let mut errs = Vec::new();
    let tokens = name_tokens(patient_name);
    for (path, s) in &all {
        for m in SEASON_MARKERS {
            let hit = if m.chars().next().is_some_and(|c| c.is_uppercase()) {
                s.to_lowercase().contains(&m.to_lowercase())
            } else {
                s.contains(m)
            };
            if hit {
                errs.push(format!("season marker {m:?} at {path}"));
            }
        }
        if has_episode_marker(s) {
            errs.push(format!("season marker EPn at {path}"));
        }
        let low = s.to_lowercase();
        for t in &tokens {
            if t.is_ascii() {
                // whole word, so a patient called Grace does not forbid the word
                let hit = low.match_indices(t.as_str()).any(|(i, _)| {
                    let before = low[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
                    let after = low[i + t.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
                    before && after
                });
                if hit {
                    errs.push(format!("patient name token {t:?} at {path}"));
                }
            } else if low.contains(t.as_str()) {
                errs.push(format!("patient name token {t:?} at {path}"));
            }
        }
    }
    errs.sort();
    errs.dedup();
    errs
}

fn run_untreated(sce: &Sce, bound_sec: f64) -> Result<f64, String> {
    let mut st = SceState::new(sce.clone());
    let mut t = 0.0;
    while t < bound_sec {
        st.tick(1.0);
        t += 1.0;
        if let Some(id) = st.outcome_id() {
            let kind = sce.outcomes.iter().find(|o| o.id == id).map(|o| o.kind.as_str()).unwrap_or("?");
            if kind == "death" {
                return Ok(t);
            }
            return Err(format!("untreated, the scenario reached {id} ({kind}) at {t} s instead of a death"));
        }
    }
    Err(format!("untreated, the patient is still alive at {bound_sec} s — the archetype's bound"))
}

fn run_path(sce: &Sce, path: &[PathStep], bound_sec: f64) -> Result<(SceState, f64), String> {
    let mut st = SceState::new(sce.clone());
    let mut t = 0.0;
    for step in path {
        while t < step.t_sec {
            st.tick(1.0);
            t += 1.0;
            if st.outcome().is_some() {
                return Err(format!("the management path ended at {t} s in {:?} before {} was given", st.outcome_id(), step.id));
            }
        }
        if !st.is_intervention(&step.id) {
            return Err(format!("the management path names {}, which the scenario does not define", step.id));
        }
        st.apply_id(&step.id);
    }
    while t < bound_sec {
        if st.outcome().is_some() {
            break;
        }
        st.tick(1.0);
        t += 1.0;
    }
    Ok((st, t))
}

/// Everything the gate checks. `pack` is the whole pack as JSON so the scans see every string.
pub fn validate(
    pack: &Value,
    sce_json: &str,
    rubric_json: &str,
    a: Archetype,
    patient_name: &str,
    path: &[PathStep],
) -> Result<Proof, String> {
    let sce = Sce::from_json(sce_json).map_err(|e| format!("the engine does not parse the scenario: {e}"))?;
    let errs = sce.validate();
    if !errs.is_empty() {
        return Err(format!("the scenario does not validate: {}", errs.join("; ")));
    }

    let untreated_death_sec = run_untreated(&sce, a.death_bound_sec())?;

    let (st, win_sec) = run_path(&sce, path, a.death_bound_sec() + 30.0 * 60.0)?;
    let win_outcome = match st.outcome_id() {
        Some(id) => {
            let kind = sce.outcomes.iter().find(|o| o.id == id).map(|o| o.kind.as_str()).unwrap_or("?");
            if kind != "win" {
                return Err(format!("the management path reached {id} ({kind}) at {win_sec} s, not a win"));
            }
            id.to_string()
        }
        None => return Err(format!("the management path reached no outcome by {win_sec} s")),
    };
    if !st.harm_events.is_empty() {
        return Err(format!("the management path recorded harm: {:?}", st.harm_events));
    }

    // the rubric: parses, every needle resolves, and the path clears the bar
    let rubric: vitals_osce::Rubric = serde_json::from_str(rubric_json).map_err(|e| format!("the scorer does not parse the rubric: {e}"))?;
    let ids: Vec<&str> = sce.interventions.iter().map(|i| i.id.as_str()).collect();
    let outcomes: Vec<&str> = sce.outcomes.iter().map(|o| o.id.as_str()).collect();
    let mut harms: Vec<String> = sce.interventions.iter().filter_map(|i| i.harm.clone()).collect();
    fn collect_harms(es: &[vitals_sce::schema::Effect], out: &mut Vec<String>) {
        for e in es {
            match e {
                vitals_sce::schema::Effect::Harm { harm } => out.push(harm.clone()),
                vitals_sce::schema::Effect::Branch { branch, els } => {
                    for arm in branch {
                        collect_harms(&arm.then, out);
                    }
                    collect_harms(els, out);
                }
                _ => {}
            }
        }
    }
    for t in &sce.triggers {
        collect_harms(&t.doo, &mut harms);
    }
    for it in &rubric.items {
        match &it.check {
            vitals_osce::Check::Action { needle, .. } | vitals_osce::Check::ActionBy { needle, .. } => {
                if !ids.iter().any(|id| id.contains(needle.as_str())) {
                    return Err(format!("rubric needle {needle:?} names no intervention"));
                }
            }
            vitals_osce::Check::ActionAny { any_of, .. } => {
                for n in any_of {
                    if !ids.iter().any(|id| id.contains(n.as_str())) {
                        return Err(format!("rubric needle {n:?} names no intervention"));
                    }
                }
            }
            vitals_osce::Check::Outcome { any_of, .. } => {
                for n in any_of {
                    if !outcomes.contains(&n.as_str()) {
                        return Err(format!("rubric outcome {n:?} is not an outcome of the scenario"));
                    }
                }
            }
            vitals_osce::Check::NoHarm { needle, .. } => {
                if !harms.iter().any(|h| h.contains(needle.as_str())) {
                    return Err(format!("rubric no_harm needle {needle:?} matches no harm the scenario can fire"));
                }
            }
            vitals_osce::Check::NoUnindicated { allow, .. } => {
                for n in allow {
                    if !ids.contains(&n.as_str()) {
                        return Err(format!("rubric allow entry {n:?} names no intervention"));
                    }
                }
            }
        }
    }
    let det = vitals_osce::score(st.events(), &rubric, st.outcome());
    if det.max != 40 {
        return Err(format!("the rubric is out of {} rather than 40", det.max));
    }
    if !det.cleared(&rubric) {
        let misses: Vec<String> = det.items.iter().filter(|i| !i.earned).map(|i| i.label.clone()).collect();
        return Err(format!("the management path scores {}/{} on its own rubric and does not pass: missed {misses:?}", det.earned, det.max));
    }

    let leaks = scan(pack, patient_name);
    if !leaks.is_empty() {
        return Err(format!("the pack leaks: {}", leaks.join("; ")));
    }

    Ok(Proof {
        untreated_death_sec,
        win_path: path.to_vec(),
        win_sec,
        win_outcome,
        golden_score: GoldenScore { earned: det.earned, max: det.max, pass_bps: rubric.pass_bps },
    })
}
