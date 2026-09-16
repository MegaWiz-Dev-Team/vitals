//! The scenario an archetype writes for one case: two states, a handful of triggers, the death
//! and the win — parameterised by the starting vitals and by which roles the plan named.
//!
//! One shape for every archetype: `presenting` (deteriorating, with a `critical` band halfway to
//! death) → `stabilising` (improving) once every critical role is done, then a win after the
//! recovery time; a death edge out of `presenting` when the archetype's death variable crosses
//! its threshold. Rates are derived, not tuned: each variable moves from its starting value to
//! its untreated target in exactly `death_minutes`, so the untreated timeline is a property of
//! the archetype and the reviewer reads one number per archetype rather than one per case.

use crate::archetype::{Archetype, Kind};
use crate::embla::{Case, Vitals0};
use crate::interventions::Built;
use crate::plan::Mapped;
use serde_json::{json, Value};

/// One vital's untreated course and its recovery.
struct Traj {
    var: &'static str,
    /// Where it is heading untreated (reached at `death_minutes`).
    target: f64,
    /// Where recovery takes it.
    recover_to: f64,
    /// A flag that, once set, stops the untreated movement (the airway taken over stops the
    /// saturation falling; dextrose stops the consciousness fading).
    stopped_by: Option<&'static str>,
}

/// The variable whose threshold ends the case, and the threshold.
struct Death {
    var: &'static str,
    below: f64,
}

struct Shape {
    trajs: Vec<Traj>,
    deaths: Vec<Death>,
    /// Custom axes beyond the seven vitals.
    variables: Vec<(&'static str, f64, f64, f64)>,
    /// Which variable the `critical` band and the late-order penalties read.
    primary: &'static str,
}

fn shape(a: Archetype, v: &Vitals0) -> Shape {
    let gcs0 = v.gcs as f64;
    match a {
        Archetype::SepticShock | Archetype::HaemorrhagicShock => Shape {
            trajs: vec![
                Traj { var: "sbp", target: 45.0, recover_to: 105.0, stopped_by: None },
                Traj { var: "hr", target: (v.hr + 40.0).min(170.0), recover_to: 95.0, stopped_by: None },
                Traj { var: "spo2", target: (v.spo2 - 10.0).max(84.0), recover_to: 97.0, stopped_by: None },
                Traj { var: "rr", target: (v.rr + 8.0).min(40.0), recover_to: 18.0, stopped_by: None },
            ],
            deaths: vec![Death { var: "sbp", below: 50.0 }],
            variables: vec![],
            primary: "sbp",
        },
        Archetype::CardiogenicShock => Shape {
            trajs: vec![
                Traj { var: "sbp", target: 45.0, recover_to: 100.0, stopped_by: None },
                Traj { var: "spo2", target: 70.0, recover_to: 96.0, stopped_by: None },
                Traj { var: "hr", target: (v.hr + 35.0).min(165.0), recover_to: 100.0, stopped_by: None },
                Traj { var: "rr", target: (v.rr + 10.0).min(40.0), recover_to: 20.0, stopped_by: None },
            ],
            deaths: vec![Death { var: "sbp", below: 50.0 }, Death { var: "spo2", below: 75.0 }],
            variables: vec![],
            primary: "sbp",
        },
        Archetype::NeuromuscularRespiratoryFailure => Shape {
            trajs: vec![
                Traj { var: "spo2", target: 60.0, recover_to: 97.0, stopped_by: Some("airway_given") },
                Traj { var: "rr", target: 6.0, recover_to: 16.0, stopped_by: Some("airway_given") },
                Traj { var: "hr", target: (v.hr + 35.0).min(150.0), recover_to: 90.0, stopped_by: None },
            ],
            deaths: vec![Death { var: "spo2", below: 65.0 }],
            variables: vec![],
            primary: "spo2",
        },
        Archetype::CnsDepressionHypoglycaemia => Shape {
            trajs: vec![
                Traj { var: "neuro", target: 3.0, recover_to: 15.0, stopped_by: Some("dextrose_given") },
                Traj { var: "spo2", target: 65.0, recover_to: 97.0, stopped_by: Some("airway_given") },
                Traj { var: "hr", target: (v.hr + 30.0).min(160.0), recover_to: 95.0, stopped_by: None },
                // Slow, unguarded: the infection behind the coma keeps taking the pressure down
                // until the specific therapy turns it. Reached at 2.5 × death_minutes.
                Traj { var: "sbp", target: v.sbp - (v.sbp - 45.0) / 2.5, recover_to: 105.0, stopped_by: None },
            ],
            deaths: vec![Death { var: "neuro", below: 3.5 }, Death { var: "spo2", below: 70.0 }, Death { var: "sbp", below: 50.0 }],
            variables: vec![("neuro", gcs0, 3.0, 15.0)],
            primary: "neuro",
        },
        Archetype::PaediatricCompensatedShock => Shape {
            trajs: vec![
                Traj { var: "sbp", target: 55.0, recover_to: 100.0, stopped_by: None },
                // Written explicitly so the engine hands the diastolic to the case: it holds
                // while the systolic falls — the narrowing pulse pressure that is the sign —
                // and comes down again as the child fills.
                Traj { var: "dbp", target: v.dbp, recover_to: 60.0, stopped_by: None },
                Traj { var: "hr", target: (v.hr + 40.0).min(195.0), recover_to: 110.0, stopped_by: None },
                Traj { var: "spo2", target: (v.spo2 - 8.0).max(86.0), recover_to: 98.0, stopped_by: None },
                Traj { var: "rr", target: (v.rr + 12.0).min(50.0), recover_to: 24.0, stopped_by: None },
            ],
            deaths: vec![Death { var: "sbp", below: 60.0 }],
            variables: vec![("fluid_load", 0.0, 0.0, 12.0)],
            primary: "sbp",
        },
        Archetype::HypoxicRespiratoryFailure => Shape {
            trajs: vec![
                Traj { var: "spo2", target: 60.0, recover_to: 96.0, stopped_by: None },
                Traj { var: "rr", target: (v.rr + 14.0).min(45.0), recover_to: 18.0, stopped_by: None },
                Traj { var: "hr", target: (v.hr + 30.0).min(150.0), recover_to: 95.0, stopped_by: None },
            ],
            deaths: vec![Death { var: "spo2", below: 65.0 }],
            variables: vec![],
            primary: "spo2",
        },
    }
}

fn start(v: &Vitals0, var: &str) -> f64 {
    match var {
        "hr" => v.hr,
        "sbp" => v.sbp,
        "dbp" => v.dbp,
        "spo2" => v.spo2,
        "rr" => v.rr,
        "temp" => v.temp,
        "gcs" | "neuro" => v.gcs as f64,
        _ => 0.0,
    }
}

/// Which win this case earns: intensive care for an emergency at the higher tiers, discharge for
/// everything gentler.
pub fn win_outcome(case: &Case) -> &'static str {
    let setting = case.meta.care_setting.as_deref().unwrap_or("").to_lowercase();
    let tier = case.meta.clinical_tier.unwrap_or(3);
    if (setting == "er" || setting == "ward" || setting == "ed") && tier >= 4 {
        "win_icu"
    } else {
        "win_discharge"
    }
}

/// The delay after which a timed critical order counts as late: the sentence's own time where
/// it fits inside the shift, else the moment the patient turns critical.
pub fn by_sec(a: Archetype, named_sec: f64) -> f64 {
    let critical_at = a.death_minutes() * 60.0 / 2.0;
    named_sec.min(critical_at)
}

pub struct Sim {
    pub sce: Value,
    pub win: &'static str,
    /// `(role id, by_sec)` for every critical role a sentence pinned to the clock.
    pub late: Vec<(String, f64)>,
    /// The harm sentences the triggers can fire, for the rubric's `no_harm` items.
    pub trigger_harms: Vec<(String, String)>,
}

pub fn build(case: &Case, a: Archetype, v: &Vitals0, mapped: &Mapped, built: &Built) -> Sim {
    let sh = shape(a, v);
    let tm = a.death_minutes();
    let recover_min = 6.0;
    let win = win_outcome(case);

    let cmp = |var: &str, op: &str, value: f64| json!({ "var": var, "op": op, "value": value });
    let death_cond = json!({ "any": sh.deaths.iter().map(|d| cmp(d.var, "lt", d.below)).collect::<Vec<_>>() });
    let primary_death = sh.deaths.iter().find(|d| d.var == sh.primary).map(|d| d.below).unwrap_or(0.0);
    let primary0 = start(v, sh.primary);
    let critical_at = (primary0 + primary_death) / 2.0;

    // presenting: every trajectory moves to its target in `tm` minutes
    let mut dyn_pres: Vec<Value> = Vec::new();
    for t in &sh.trajs {
        let s0 = start(v, t.var);
        let rate = (t.target - s0) / tm;
        let mut d = json!({ "var": t.var, "rate_per_min": rate });
        if rate < 0.0 { d["floor"] = json!(t.target) } else { d["ceil"] = json!(t.target) }
        if let Some(f) = t.stopped_by {
            d["when"] = json!({ "flag": f, "is": false });
        }
        dyn_pres.push(d);
    }
    // stabilising: back toward recovery values in `recover_min`
    let mut dyn_rec: Vec<Value> = Vec::new();
    for t in &sh.trajs {
        let span = (t.recover_to - t.target).abs().max(1.0);
        let rate = if t.recover_to >= t.target { span / recover_min } else { -span / recover_min };
        let mut d = json!({ "var": t.var, "rate_per_min": rate });
        if rate < 0.0 { d["floor"] = json!(t.recover_to) } else { d["ceil"] = json!(t.recover_to) }
        dyn_rec.push(d);
    }

    let critical: Vec<String> = mapped.critical().iter().map(|p| p.tx_id()).collect();
    let turn = json!({ "all": critical.iter().map(|id| json!({ "done": id })).collect::<Vec<_>>() });

    let states = json!([
        {
            "id": "presenting",
            "status": "deteriorating",
            "dynamics": dyn_pres,
            "bands": [ { "status": "critical", "when": cmp(sh.primary, "lt", critical_at) } ],
            "transitions": [
                { "outcome": "death_arrest", "when": death_cond },
                { "to_state": "stabilising", "when": turn }
            ]
        },
        {
            "id": "stabilising",
            "status": "improving",
            "dynamics": dyn_rec,
            "bands": [],
            "transitions": []
        }
    ]);

    let mut triggers: Vec<Value> = Vec::new();
    let mut trigger_harms: Vec<(String, String)> = Vec::new();

    // the win
    triggers.push(json!({
        "id": "recovered",
        "once": true,
        "when": { "all": [ { "in_state": "stabilising" }, { "var": "t_in_state", "op": "ge", "value": a.recovery_sec() } ] },
        "do": [ { "outcome": win } ]
    }));

    // consciousness: a stepped ladder, because the engine rounds GCS to a whole number each tick
    // and a slow continuous rate never moves it
    let gcs0 = v.gcs as f64;
    if a == Archetype::CnsDepressionHypoglycaemia {
        let mut k = 1.0;
        while gcs0 - k >= 3.0 {
            triggers.push(json!({
                "id": format!("gcs_down_{}", k as u8),
                "once": true,
                "when": { "all": [ { "in_state": "presenting" }, { "var": "neuro", "op": "le", "value": gcs0 - k } ] },
                "do": [ { "set": { "gcs": gcs0 - k } } ]
            }));
            k += 1.0;
        }
    } else {
        let obtunded = (gcs0 - 3.0).max(8.0).min(gcs0);
        if obtunded < gcs0 {
            triggers.push(json!({
                "id": "obtunded",
                "once": true,
                "when": { "all": [ { "in_state": "presenting" }, cmp(sh.primary, "lt", critical_at) ] },
                "do": [ { "set": { "gcs": obtunded } }, { "beat": "the answers come slower — the eyes open only to a voice" } ]
            }));
        }
    }
    triggers.push(json!({
        "id": "wakes",
        "once": true,
        "when": { "all": [ { "in_state": "stabilising" }, { "var": "t_in_state", "op": "ge", "value": 150.0 } ] },
        "do": [ { "set": { "gcs": 15.0 } }, { "beat": "awake, oriented, asking what happened" } ]
    }));

    // late orders: a critical role a sentence pinned to the clock costs harm when the clock passes
    let mut late: Vec<(String, f64)> = Vec::new();
    for p in mapped.critical() {
        let Some(t) = mapped.timed.get(p.role.id) else { continue };
        let by = by_sec(a, t.named_sec);
        let text = format!("{} delayed past the window it had to happen in", p.role.label.to_lowercase());
        let nudge = match sh.primary {
            "sbp" => json!({ "delta": { "sbp": -6.0 }, "floor": 30.0 }),
            "spo2" => json!({ "delta": { "spo2": -4.0 }, "floor": 40.0 }),
            _ => json!({ "delta": { "neuro": -1.0 }, "floor": 3.0 }),
        };
        triggers.push(json!({
            "id": format!("late_{}", p.role.id),
            "once": true,
            "when": { "all": [ { "in_state": "presenting" }, { "var": "t_elapsed", "op": "ge", "value": by }, { "not": { "done": p.tx_id() } } ] },
            "do": [ { "harm": text }, nudge ]
        }));
        trigger_harms.push((format!("late_{}", p.role.id), text));
        late.push((p.role.id.to_string(), by));
    }

    // gates: hands-on care before the gate is harm
    for g in mapped.gates() {
        let hands_on: Vec<Value> = built
            .exams
            .iter()
            .map(|(id, _, _)| json!({ "done": id }))
            .chain(built.ix.iter().map(|(id, _)| json!({ "done": id })))
            .collect();
        if hands_on.is_empty() {
            continue;
        }
        let text = format!("hands-on care before {} — staff exposed", g.role.label.to_lowercase());
        triggers.push(json!({
            "id": format!("skipped_{}", g.role.id),
            "once": true,
            "when": { "all": [ { "not": { "done": g.tx_id() } }, { "any": hands_on } ] },
            "do": [ { "harm": text } ]
        }));
        trigger_harms.push((format!("skipped_{}", g.role.id), text));
    }

    // too much of a good thing: the child who is filled past the leak
    if a == Archetype::PaediatricCompensatedShock {
        let text = "fluid overload — puffy eyelids, a rising respiratory rate, crackles, the effusion grows".to_string();
        triggers.push(json!({
            "id": "fluid_overload",
            "once": true,
            "when": { "var": "fluid_load", "op": "ge", "value": 4.0 },
            "do": [ { "harm": text }, { "delta": { "spo2": -6.0 }, "floor": 80.0 }, { "delta": { "rr": 8.0 }, "cap": 60.0 } ]
        }));
        trigger_harms.push(("fluid_overload".into(), text));
    }

    let variables: serde_json::Map<String, Value> = sh
        .variables
        .iter()
        .map(|(name, init, min, max)| (name.to_string(), json!({ "init": init, "min": min, "max": max })))
        .collect();

    let setting = match case.meta.care_setting.as_deref().unwrap_or("").to_lowercase().as_str() {
        "er" | "ed" => "ED",
        "ward" => "ward",
        _ => "OPD",
    };

    let win_label = match win {
        "win_icu" => "Treated in time — handed over to intensive care, improving",
        _ => "Treated in time — observed and discharged",
    };

    let sce = json!({
        "_note": format!(
            "Compiled by vitals-casefactory from embla-cases {} under the {} archetype. Provisional: clinically shaped by a deterministic compiler, not clinically reviewed. Untreated the patient reaches death in about {} sim minutes; every critical order in the plan turns it.",
            case.meta.id, a.id(), tm
        ),
        "setting": setting,
        "tick_seconds": 1.0,
        "vitals0": { "hr": v.hr, "sbp": v.sbp, "dbp": v.dbp, "spo2": v.spo2, "rr": v.rr, "temp": v.temp, "gcs": v.gcs },
        "variables": variables,
        "initial_state": "presenting",
        "states": states,
        "interventions": built.interventions,
        "triggers": triggers,
        "outcomes": [
            { "id": win, "kind": "win", "label": win_label },
            { "id": "death_arrest", "kind": "death", "label": "Untreated too long — the arrest could not be reversed" }
        ],
        "debrief": {
            "expect": mapped.present.iter().filter(|p| matches!(p.role.kind, Kind::Critical | Kind::Gate)).map(|p| {
                let mut e = json!({ "id": p.tx_id(), "label": p.role.label });
                if let Some((_, by)) = late.iter().find(|(id, _)| id == p.role.id) {
                    e["within_sec"] = json!(by);
                }
                e
            }).collect::<Vec<_>>(),
            "avoid": mapped.harmful().iter().map(|p| json!({ "id": p.tx_id(), "label": p.role.label, "why": p.role.harm })).collect::<Vec<_>>()
        }
    });

    Sim { sce, win, late, trigger_harms }
}
