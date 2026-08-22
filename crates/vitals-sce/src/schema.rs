//! The SCE schema — the on-disk form of a scenario.
//!
//! A scenario is a hybrid automaton: initial vitals, named states with continuous per-second
//! dynamics, global triggers, discrete transitions, free-text interventions, and terminal
//! outcomes. This module is the data; `crate::runtime` is the interpreter.
//!
//! Ported from Embla's `engine/src/sce.rs`. The two are held together by
//! `conformance/ep1-vectors.json`, not by a shared build.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

fn default_true() -> bool { true }
fn default_tick() -> f64 { 1.0 }

/// Comparison operator for a [`Cond::Cmp`] leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op { Lt, Le, Gt, Ge, Eq, Ne }

/// A boolean condition over the live sim state. Leaves read pseudo-variables
/// (`t_in_state` = seconds in the current state, `t_elapsed` = since start)
/// alongside vitals/custom axes, so time needs no special variant. Untagged: each
/// variant carries a disjoint required key, so the JSON is unambiguous.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Cond {
    /// every sub-condition true
    All { all: Vec<Cond> },
    /// any sub-condition true
    Any { any: Vec<Cond> },
    /// negation
    Not { not: Box<Cond> },
    /// numeric compare `var op value` (var ∈ vitals | custom axis | t_in_state | t_elapsed)
    Cmp { var: String, op: Op, value: f64 },
    /// a flag is set (or, with `"is": false`, cleared)
    Flag { flag: String, #[serde(default = "default_true")] is: bool },
    /// the automaton is currently in this state
    InState { in_state: String },
    /// an intervention with this id has been applied at least once
    Done { done: String },
}

/// One mutation / flag / beat / transition / branch a state, trigger or
/// intervention can fire. Untagged on a disjoint key.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Effect {
    /// additive change to one or more vars, each clamped by the optional cap/floor
    Delta { delta: BTreeMap<String, f64>, #[serde(default)] cap: Option<f64>, #[serde(default)] floor: Option<f64> },
    /// absolute set of one or more vars
    Set { set: BTreeMap<String, f64> },
    /// set/clear a flag; with `for_sec`, it auto-clears after that many sim seconds
    Flag { flag: String, #[serde(default = "default_true")] value: bool, #[serde(default)] for_sec: Option<f64> },
    /// emit `NarrativeBeat::Threshold(name)` for the Director / monitor
    Beat { beat: String },
    /// record a harm event + emit `NarrativeBeat::Harm(name)`
    Harm { harm: String },
    /// force a state transition
    ToState { to_state: String },
    /// terminate the encounter with an outcome id
    Outcome { outcome: String },
    /// run the first arm whose `if` holds, else the `else` effects
    Branch { branch: Vec<Arm>, #[serde(default, rename = "else")] els: Vec<Effect> },
}

/// One `{ "if": <cond>, "then": [<effects>] }` arm of an [`Effect::Branch`].
#[derive(Debug, Clone, Deserialize)]
pub struct Arm {
    #[serde(rename = "if")] pub iff: Cond,
    pub then: Vec<Effect>,
}

/// Initial vital signs at t=0 — the standard patient-monitor vector.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Vitals0 {
    pub hr: f64,
    pub sbp: f64,
    pub dbp: f64,
    pub spo2: f64,
    pub rr: f64,
    pub temp: f64,
    pub gcs: u8,
}

/// A custom hidden physiologic axis (e.g. `airway_patency`, `perfusion`).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Axis {
    pub init: f64,
    #[serde(default)] pub min: Option<f64>,
    #[serde(default)] pub max: Option<f64>,
}

/// Continuous per-second change of one var while resident in a state.
#[derive(Debug, Clone, Deserialize)]
pub struct Dynamic {
    pub var: String,
    /// signed rate; the interpreter applies `rate_per_min/60 * dt` each tick
    pub rate_per_min: f64,
    /// guard — apply only while this condition holds (e.g. not flag `adrenaline_active`)
    #[serde(default)] pub when: Option<Cond>,
    #[serde(default)] pub floor: Option<f64>,
    #[serde(default)] pub ceil: Option<f64>,
}

/// A finer status label that overrides the state's default while `when` holds.
/// Bands are evaluated top-to-bottom; first match wins.
#[derive(Debug, Clone, Deserialize)]
pub struct Band {
    pub status: String,
    pub when: Cond,
}

/// A discrete edge out of a state — to another state or to a terminal outcome.
#[derive(Debug, Clone, Deserialize)]
pub struct Transition {
    #[serde(default)] pub to_state: Option<String>,
    #[serde(default)] pub outcome: Option<String>,
    pub when: Cond,
    /// optional effects fired on taking the edge
    #[serde(default, rename = "do")] pub doo: Vec<Effect>,
}

/// A node of the automaton.
#[derive(Debug, Clone, Deserialize)]
pub struct State {
    pub id: String,
    /// clinical/display status while resident here (stable|deteriorating|critical|improving|recovered|arrest)
    #[serde(default)] pub status: Option<String>,
    /// cardiac rhythm while resident here (sinus|vf|vt|pea|asystole). Absent means unchanged —
    /// only the states where the rhythm is part of the teaching need to say anything.
    #[serde(default)] pub rhythm: Option<String>,
    #[serde(default)] pub dynamics: Vec<Dynamic>,
    #[serde(default)] pub bands: Vec<Band>,
    #[serde(default)] pub transitions: Vec<Transition>,
}

/// Free-text → intervention matcher (generalises `physiology::classify`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Matcher {
    /// matches if the (lower-cased) action text contains ANY of these keywords
    #[serde(default)] pub any_kw: Vec<String>,
    /// AND every group must have ≥1 keyword hit (e.g. `[["adren"],["iv","push"]]`)
    #[serde(default)] pub all_groups: Vec<Vec<String>>,
    /// …but NONE of these (e.g. exclude iv/push from the IM variant)
    #[serde(default)] pub not_kw: Vec<String>,
}

/// A clinical action the trainee can take, matched from free text, with its effect.
#[derive(Debug, Clone, Deserialize)]
pub struct Intervention {
    pub id: String,
    #[serde(default)] pub label: Option<String>,
    #[serde(rename = "match")] pub matcher: Matcher,
    #[serde(default)] pub effects: Vec<Effect>,
    /// harm recorded + beat emitted when applied (e.g. IV-push 1:1000 adrenaline)
    #[serde(default)] pub harm: Option<String>,
    /// Equipment this intervention LEAVES ON the patient, if any (`"o2"`, `"iv"`, `"ett"`).
    ///
    /// Without this, saying "give oxygen" moved the physiology but put nothing on the bedside,
    /// while pressing the oxygen button did both — one fact with two records, which is the exact
    /// split `EquipmentSet` exists to close.
    #[serde(default)] pub equipment: Option<String>,
    /// What the setting reads when the intervention is what attached it. The case's own canonical
    /// dose: a learner who says "give oxygen" without a number is asking for what this scenario
    /// teaches, and the flowmeter has to show something a learner can check.
    #[serde(default)] pub equipment_setting: Option<f64>,
}

/// A global rule evaluated each tick (e.g. biphasic onset). `once` fires it at most once.
#[derive(Debug, Clone, Deserialize)]
pub struct Trigger {
    pub id: String,
    pub when: Cond,
    #[serde(default, rename = "do")] pub doo: Vec<Effect>,
    #[serde(default = "default_true")] pub once: bool,
}

/// A terminal outcome class. `kind` ∈ win | death | transfer (UI/scoring maps it).
#[derive(Debug, Clone, Deserialize)]
pub struct OutcomeDef {
    pub id: String,
    pub kind: String,
    #[serde(default)] pub label: Option<String>,
}

/// The whole time-evolving scenario — the optional `scenario` block of a Case.
#[derive(Debug, Clone, Deserialize)]
pub struct Sce {
    /// sim-clock granularity in seconds (interpreter hint; default 1.0)
    #[serde(default = "default_tick")] pub tick_seconds: f64,
    /// room/monitor hint: ED | ICU | OPD | ward
    #[serde(default)] pub setting: Option<String>,
    pub vitals0: Vitals0,
    /// custom hidden axes beyond the 7 standard vitals
    #[serde(default)] pub variables: BTreeMap<String, Axis>,
    pub initial_state: String,
    pub states: Vec<State>,
    #[serde(default)] pub interventions: Vec<Intervention>,
    #[serde(default)] pub triggers: Vec<Trigger>,
    #[serde(default)] pub outcomes: Vec<OutcomeDef>,
}

impl Sce {
    pub fn from_json(s: &str) -> Result<Sce, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// Variable names the engine recognises: the 7 vitals + the two pseudo-clocks +
    /// every declared custom axis.
    pub fn known_vars(&self) -> BTreeSet<String> {
        let mut v: BTreeSet<String> = ["hr", "sbp", "dbp", "spo2", "rr", "temp", "gcs", "t_in_state", "t_elapsed"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        v.extend(self.variables.keys().cloned());
        v
    }

    /// Referential-integrity check (authoring guardrail). Empty result = valid; the
    /// runtime interpreter may then assume every state/outcome/var reference resolves.
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let states: BTreeSet<&str> = self.states.iter().map(|s| s.id.as_str()).collect();
        let outcomes: BTreeSet<&str> = self.outcomes.iter().map(|o| o.id.as_str()).collect();
        let vars = self.known_vars();

        if self.states.is_empty() {
            errs.push("no states defined".into());
        }
        if !states.contains(self.initial_state.as_str()) {
            errs.push(format!("initial_state '{}' is not a defined state", self.initial_state));
        }
        if states.len() != self.states.len() {
            errs.push("duplicate state id(s)".into());
        }

        for st in &self.states {
            for d in &st.dynamics {
                if !vars.contains(&d.var) {
                    errs.push(format!("state '{}': dynamics references unknown var '{}'", st.id, d.var));
                }
                cond_vars(d.when.as_ref(), &vars, &format!("state '{}' dynamics.when", st.id), &mut errs);
            }
            for b in &st.bands {
                cond_vars(Some(&b.when), &vars, &format!("state '{}' band.when", st.id), &mut errs);
            }
            for t in &st.transitions {
                match (&t.to_state, &t.outcome) {
                    (None, None) => errs.push(format!("state '{}': a transition has neither to_state nor outcome", st.id)),
                    (Some(s), _) if !states.contains(s.as_str()) => {
                        errs.push(format!("state '{}': transition to undefined state '{}'", st.id, s))
                    }
                    (None, Some(o)) if !outcomes.contains(o.as_str()) => {
                        errs.push(format!("state '{}': transition to undefined outcome '{}'", st.id, o))
                    }
                    _ => {}
                }
                cond_vars(Some(&t.when), &vars, &format!("state '{}' transition.when", st.id), &mut errs);
                walk_effects(&t.doo, &format!("state '{}' transition.do", st.id), &states, &outcomes, &vars, &mut errs);
            }
        }
        for tr in &self.triggers {
            cond_vars(Some(&tr.when), &vars, &format!("trigger '{}' when", tr.id), &mut errs);
            walk_effects(&tr.doo, &format!("trigger '{}' do", tr.id), &states, &outcomes, &vars, &mut errs);
        }
        for iv in &self.interventions {
            walk_effects(&iv.effects, &format!("intervention '{}' effects", iv.id), &states, &outcomes, &vars, &mut errs);
        }
        errs
    }
}

/// Recursively check every var referenced by a condition is known.
fn cond_vars(c: Option<&Cond>, vars: &BTreeSet<String>, where_: &str, errs: &mut Vec<String>) {
    let Some(c) = c else { return };
    match c {
        Cond::All { all } => all.iter().for_each(|x| cond_vars(Some(x), vars, where_, errs)),
        Cond::Any { any } => any.iter().for_each(|x| cond_vars(Some(x), vars, where_, errs)),
        Cond::Not { not } => cond_vars(Some(not), vars, where_, errs),
        Cond::Cmp { var, .. } => {
            if !vars.contains(var) {
                errs.push(format!("{where_}: unknown var '{var}'"));
            }
        }
        Cond::Flag { .. } | Cond::InState { .. } | Cond::Done { .. } => {}
    }
}

/// Recursively check every state/outcome/var an effect tree references resolves.
fn walk_effects(
    es: &[Effect],
    where_: &str,
    states: &BTreeSet<&str>,
    outcomes: &BTreeSet<&str>,
    vars: &BTreeSet<String>,
    errs: &mut Vec<String>,
) {
    for e in es {
        match e {
            Effect::Delta { delta, .. } => {
                for k in delta.keys() {
                    if !vars.contains(k) {
                        errs.push(format!("{where_}: delta references unknown var '{k}'"));
                    }
                }
            }
            Effect::Set { set } => {
                for k in set.keys() {
                    if !vars.contains(k) {
                        errs.push(format!("{where_}: set references unknown var '{k}'"));
                    }
                }
            }
            Effect::ToState { to_state } => {
                if !states.contains(to_state.as_str()) {
                    errs.push(format!("{where_}: to_state '{to_state}' undefined"));
                }
            }
            Effect::Outcome { outcome } => {
                if !outcomes.contains(outcome.as_str()) {
                    errs.push(format!("{where_}: outcome '{outcome}' undefined"));
                }
            }
            Effect::Branch { branch, els } => {
                for a in branch {
                    cond_vars(Some(&a.iff), vars, where_, errs);
                    walk_effects(&a.then, where_, states, outcomes, vars, errs);
                }
                walk_effects(els, where_, states, outcomes, vars, errs);
            }
            Effect::Flag { .. } | Effect::Beat { .. } | Effect::Harm { .. } => {}
        }
    }
}
