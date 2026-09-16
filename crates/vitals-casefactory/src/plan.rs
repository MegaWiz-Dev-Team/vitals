//! The management plan, read against the archetype: which roles this case names, which orders
//! it forbids, and which steps the compiler could not place.

use crate::archetype::{Archetype, Kind, Role, COMMON, NEGATED_HARMS};
use crate::embla::Case;
use crate::text::{fragments, negated, time_named};
use serde::Serialize;
use std::collections::BTreeMap;

/// A role the case actually has, and where it came from.
#[derive(Debug, Clone)]
pub struct Present {
    pub role: Role,
    /// Index of the plan step that first named it; `None` for a role every case carries or a
    /// harm the archetype carries whatever the plan says.
    pub source: Option<usize>,
}

impl Present {
    pub fn tx_id(&self) -> String {
        format!("tx_{}", self.role.id)
    }
}

/// One step of the case's plan, as the pack records it for the reviewer.
#[derive(Debug, Clone, Serialize)]
pub struct PlanStep {
    pub step: String,
    /// The `tx_` interventions this step compiled into. Empty means the compiler could not
    /// place the step — it is still listed, so nothing the plan says disappears silently.
    pub interventions: Vec<String>,
}

/// A time a red flag or a plan step attached to a role.
#[derive(Debug, Clone, Serialize)]
pub struct Timed {
    /// What the sentence said, in sim seconds.
    pub named_sec: f64,
    /// The sentence itself, so the reviewer can check the reading.
    pub sentence: String,
}

#[derive(Debug, Clone)]
pub struct Mapped {
    pub present: Vec<Present>,
    pub steps: Vec<PlanStep>,
    pub timed: BTreeMap<String, Timed>,
}

impl Mapped {
    pub fn critical(&self) -> Vec<&Present> {
        self.present.iter().filter(|p| p.role.kind == Kind::Critical).collect()
    }
    pub fn gates(&self) -> Vec<&Present> {
        self.present.iter().filter(|p| p.role.kind == Kind::Gate).collect()
    }
    pub fn harmful(&self) -> Vec<&Present> {
        self.present.iter().filter(|p| p.role.kind == Kind::Harmful).collect()
    }
    pub fn supportive(&self) -> Vec<&Present> {
        self.present.iter().filter(|p| p.role.kind == Kind::Supportive).collect()
    }
    pub fn get(&self, id: &str) -> Option<&Present> {
        self.present.iter().find(|p| p.role.id == id)
    }
}

/// Roles every case carries whether or not the plan spells them out.
const ALWAYS: &[&str] = &["oxygen", "admit", "monitor", "explain"];

fn positive_hit(role: &Role, sentence: &str) -> bool {
    fragments(sentence).iter().any(|f| !negated(f) && role.kw.iter().any(|k| f.contains(k)))
}

fn negated_hit(role: &Role, sentence: &str) -> bool {
    fragments(sentence).iter().any(|f| negated(f) && role.kw.iter().any(|k| f.contains(k)))
}

/// Read the plan and the red flags against the archetype.
pub fn map(case: &Case, archetype: Archetype) -> Mapped {
    let plan = &case.hidden.management_plan;
    let flags = &case.hidden.red_flags;
    let mut present: Vec<Present> = Vec::new();

    let mut add = |role: Role, source: Option<usize>| {
        if !present.iter().any(|p| p.role.id == role.id) {
            present.push(Present { role, source });
        }
    };

    // The archetype's own roles first — they are the ones with physiology behind them — then
    // the common set, then the harms. A positive fragment anywhere in the plan places a role.
    for role in archetype.roles().iter().chain(COMMON.iter()) {
        let source = plan.iter().position(|s| positive_hit(role, s));
        // Oxygen is critical in the shapes where it turns the trajectory.
        let role = if role.id == "oxygen" && archetype.oxygen_is_critical() {
            Role { kind: Kind::Critical, ..*role }
        } else {
            *role
        };
        if source.is_some() || ALWAYS.contains(&role.id) {
            add(role, source);
        }
    }
    for role in archetype.intrinsic_harms() {
        add(*role, None);
    }
    for role in NEGATED_HARMS {
        let hit = plan.iter().chain(flags.iter()).any(|s| negated_hit(role, s));
        if hit {
            add(*role, None);
        }
    }

    // What each step compiled into. Every step is listed; an empty list is the honest record
    // of a sentence the compiler could not place.
    let steps = plan
        .iter()
        .map(|s| PlanStep {
            step: s.clone(),
            interventions: present
                .iter()
                .filter(|p| match p.role.kind {
                    Kind::Harmful => negated_hit(&p.role, s),
                    _ => positive_hit(&p.role, s),
                })
                .map(Present::tx_id)
                .collect(),
        })
        .collect();

    // Times. A red flag or a plan step that names a time and a critical role in one sentence
    // pins that role to the clock; the earliest reading wins.
    let mut timed: BTreeMap<String, Timed> = BTreeMap::new();
    for s in flags.iter().chain(plan.iter()) {
        let Some(secs) = time_named(s) else { continue };
        for p in present.iter().filter(|p| matches!(p.role.kind, Kind::Critical | Kind::Gate)) {
            if positive_hit(&p.role, s) || (p.role.kind == Kind::Gate && s.to_lowercase().contains(p.role.kw[0])) {
                let e = timed.entry(p.role.id.to_string()).or_insert(Timed { named_sec: secs, sentence: s.clone() });
                if secs < e.named_sec {
                    *e = Timed { named_sec: secs, sentence: s.clone() };
                }
            }
        }
    }

    Mapped { present, steps, timed }
}
