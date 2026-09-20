//! The ACLS-shaped family: states named by rhythm, moved by the algorithm's steps and by the
//! two-minute clock.
//!
//! One arrest core — `arrest_vf` → `post_shock_cpr` → `rosc`, `arrest_pea`, `arrest_asystole` —
//! shared by four entries: a patient who arrives without a pulse, a narrow-complex tachycardia,
//! an atrial fibrillation with a rapid response, and a symptomatic bradycardia. The rhythm is the
//! engine's own per-state `rhythm` (so the monitor draws it and the kit's shock button knows what
//! it is shocking) and is also spoken in a beat on every change, so the chart says "VF", "PEA",
//! "sinus — ROSC" in words. What the engine cannot yet show is a *morphology* for a rhythm with a
//! pulse: SVT, AF and a heart block all read `sinus` with their rate on the monitor.
//!
//! Deterministic megacode: ROSC is declared at a rhythm check (two minutes after a shock, or two
//! minutes into PEA, four into asystole) when the algorithm's requirements so far are met —
//! compressions running, adrenaline in its window, at least two shocks for a shockable rhythm and
//! amiodarone once the third has been needed. Nothing is drawn from a hat.

use crate::archetype::{Archetype, Kind};
use crate::embla::{Case, Vitals0};
use crate::interventions::Built;
use crate::plan::Mapped;
use crate::scenario::{win_outcome, Sim};
use serde_json::{json, Value};

/// Where the case begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Vf,
    Pea,
    Asystole,
    TachyStable,
    TachyUnstable,
    Brady,
}

pub const ARREST_STATES: [&str; 4] = ["arrest_vf", "post_shock_cpr", "arrest_pea", "arrest_asystole"];
const TACHY_STATES: [&str; 2] = ["tachy_stable", "tachy_unstable"];

/// Seconds compressions count for once ordered — one cycle and the change-over.
const CPR_WINDOW_SEC: f64 = 150.0;
/// Seconds one dose of adrenaline counts for — the algorithm's own three-to-five minutes.
const ADRENALINE_WINDOW_SEC: f64 = 300.0;
/// The first shock is due inside the first cycle.
const FIRST_SHOCK_BY_SEC: f64 = 120.0;

pub fn entry(case: &Case, a: Archetype, v: &Vitals0) -> Entry {
    match a {
        Archetype::AclsCardiacArrest => {
            let hay = {
                let mut h = case.haystack();
                for i in &case.investigations {
                    h.push(' ');
                    h.push_str(&i.result.value.to_string().to_lowercase());
                }
                h
            };
            if crate::text::contains_kw(&hay, "asystole") {
                Entry::Asystole
            } else if crate::text::contains_kw(&hay, "pulseless electrical") || crate::text::contains_kw(&hay, "pea") {
                Entry::Pea
            } else {
                Entry::Vf
            }
        }
        Archetype::AclsBradycardia => Entry::Brady,
        _ => {
            if v.sbp < 90.0 { Entry::TachyUnstable } else { Entry::TachyStable }
        }
    }
}

fn in_any(states: &[&str]) -> Value {
    json!({ "any": states.iter().map(|s| json!({ "in_state": s })).collect::<Vec<_>>() })
}

fn arrest_entry_effects(rhythm_beat: &str) -> Vec<Value> {
    vec![
        json!({ "set": { "sbp": 0.0, "dbp": 0.0, "spo2": 0.0, "rr": 0.0, "gcs": 3.0 } }),
        json!({ "set": { "perfusion": 60.0 } }),
        json!({ "beat": rhythm_beat }),
    ]
}

fn rosc_entry_effects() -> Vec<Value> {
    vec![
        json!({ "set": { "sbp": 90.0, "dbp": 55.0, "hr": 100.0, "spo2": 90.0, "rr": 12.0 } }),
        json!({ "beat": "rhythm check — sinus, and a pulse under the fingers: ROSC" }),
    ]
}

/// The turn for a case with a pulse: the flags the plan's critical roles set.
fn turn(a: Archetype, mapped: &Mapped) -> Value {
    let flags = |ids: &[&str]| -> Vec<Value> {
        ids.iter().filter(|id| mapped.get(id).is_some()).map(|id| json!({ "flag": format!("{id}_given") })).collect()
    };
    match a {
        Archetype::AclsTachycardiaSvt => {
            let mut all = flags(&["vagal", "adenosine"]);
            if all.is_empty() {
                all = flags(&["rate_control"]);
            }
            json!({ "all": all })
        }
        Archetype::AclsTachycardiaAf => json!({ "all": flags(&["rate_control", "anticoagulation"]) }),
        Archetype::AclsBradycardia => {
            let mut all = flags(&["atropine"]);
            let second = flags(&["pacing", "chronotrope"]);
            if !second.is_empty() {
                all.push(json!({ "any": second }));
            }
            json!({ "all": all })
        }
        _ => json!({ "all": [] }),
    }
}

/// The rate the rhythm settles to once turned.
fn converted_hr(a: Archetype) -> f64 {
    match a {
        Archetype::AclsTachycardiaSvt => 88.0,
        Archetype::AclsTachycardiaAf => 95.0,
        Archetype::AclsBradycardia => 70.0,
        _ => 90.0,
    }
}

/// Words that say the heart is structurally diseased — a valve, a cardiomyopathy, a reduced
/// ejection fraction, a congenital defect, an old infarct. Read in the case's own sentences and
/// never inside a negated one, so "no structural heart disease" on the echo says what it says.
const STRUCTURAL_KW: &[&str] = &[
    "structural heart disease", "structural abnormalit*", "cardiomyopathy", "valvular", "valve disease", "mitral stenosis", "mitral regurgitation",
    "aortic stenosis", "aortic regurgitation", "rheumatic heart", "reduced ejection fraction", "reduced systolic function", "reduced ef",
    "lvef 2*", "lvef 3*", "lvef 4*", "ef 2*%", "ef 3*%", "ef 4*%", "congenital heart", "ventricular septal defect", "vsd", "atrial septal defect",
    "ischaemic heart disease", "ischemic heart disease", "coronary artery disease", "prior myocardial infarction", "previous myocardial infarction",
    "old myocardial infarction", "hypertrophic", "left ventricular hypertrophy",
];

/// Does this case carry a structural heart disease? The clinical advisor's ruling 3.2 (20 Sep
/// 2026): a converted rhythm on a structurally sound heart goes home; on a diseased one it goes
/// to intensive care.
pub fn structural_heart_disease(case: &Case) -> bool {
    let mut sentences: Vec<String> = vec![case.haystack()];
    sentences.extend(case.hidden.management_plan.iter().cloned());
    sentences.extend(case.presentation.pmh.iter().map(|p| p.display.clone()));
    for i in &case.investigations {
        sentences.push(i.result.value.to_string());
        if let Some(r) = &i.result.report {
            sentences.push(r.clone());
        }
    }
    sentences.iter().flat_map(|s| crate::text::fragments(s)).any(|f| !crate::text::negated(&f) && STRUCTURAL_KW.iter().any(|k| crate::text::contains_kw(&f, k)))
}

pub fn build(case: &Case, a: Archetype, v: &Vitals0, mapped: &Mapped, built: &Built) -> Sim {
    let e = entry(case, a, v);
    let with_pulse = !matches!(e, Entry::Vf | Entry::Pea | Entry::Asystole);
    // A tachycardia's ending is decided by how it was turned (the advisor's ruling 3.2): home
    // when the drugs converted it and the heart is sound; intensive care when it arrived
    // unstable (cardioversion was what it needed), when the heart is structurally diseased, or
    // — decided at the bedside — when the learner cardioverted it. Everything else keeps the
    // case's own ending.
    let tachy = matches!(e, Entry::TachyStable | Entry::TachyUnstable);
    let icu_fixed = tachy && (e == Entry::TachyUnstable || structural_heart_disease(case));
    let win = if tachy { if icu_fixed { "win_icu" } else { "win_discharge" } } else { win_outcome(case) };
    let wins: Vec<&'static str> = if tachy { vec!["win_discharge", "win_icu"] } else { vec![win] };

    // perfusion: the no-flow clock of an arrest. Twenty a minute with nobody on the chest,
    // four with compressions running — three minutes to nothing, or fifteen.
    let perfusion_dynamics = json!([
        { "var": "perfusion", "rate_per_min": -20.0, "floor": 0.0, "when": { "flag": "cpr_active", "is": false } },
        { "var": "perfusion", "rate_per_min": -4.0, "floor": 0.0, "when": { "flag": "cpr_active" } }
    ]);
    let dead = json!({ "outcome": "death_arrest", "when": { "var": "perfusion", "op": "le", "value": 0.0 } });
    let cpr_and_adrenaline = vec![json!({ "flag": "cpr_active" }), json!({ "flag": "adrenaline_active" })];

    let mut states: Vec<Value> = Vec::new();

    // ── entries with a pulse ─────────────────────────────────────────────────────────
    if with_pulse {
        let pre_arrest_min = a.death_minutes() - 3.0;
        let target_hr = converted_hr(a);
        let turn_edge = json!({ "to_state": "converted", "when": turn(a, mapped), "do": [ { "set": { "hr": target_hr } }, { "beat": "the rhythm breaks — sinus on the monitor, the pressure climbs" } ] });
        match e {
            Entry::TachyStable | Entry::TachyUnstable => {
                let stable_min = pre_arrest_min * 0.4;
                let unstable_min = pre_arrest_min - stable_min;
                let to_unstable_rate = (89.0 - v.sbp) / stable_min;
                let unstable_start = if e == Entry::TachyStable { 89.0 } else { v.sbp };
                let to_arrest_rate = (64.0 - unstable_start) / unstable_min;
                states.push(json!({
                    "id": "tachy_stable", "status": "deteriorating", "rhythm": "sinus",
                    "dynamics": [
                        { "var": "sbp", "rate_per_min": to_unstable_rate.min(-0.5), "floor": 85.0 },
                        { "var": "hr", "rate_per_min": 2.0, "ceil": 200.0 },
                        { "var": "spo2", "rate_per_min": -0.3, "floor": 88.0 }
                    ],
                    "bands": [],
                    "transitions": [
                        turn_edge,
                        { "to_state": "tachy_unstable", "when": { "var": "sbp", "op": "lt", "value": 90.0 },
                          "do": [ { "beat": "the pressure is down and the head swims — the rhythm is unstable now" } ] }
                    ]
                }));
                states.push(json!({
                    "id": "tachy_unstable", "status": "critical", "rhythm": "sinus",
                    "dynamics": [
                        { "var": "sbp", "rate_per_min": to_arrest_rate.min(-0.5), "floor": 60.0 },
                        { "var": "hr", "rate_per_min": 1.0, "ceil": 210.0 },
                        { "var": "spo2", "rate_per_min": -0.5, "floor": 85.0 }
                    ],
                    "bands": [],
                    "transitions": [
                        turn_edge,
                        { "to_state": "arrest_vf", "when": { "var": "sbp", "op": "lt", "value": 65.0 },
                          "do": arrest_entry_effects("rhythm: ventricular fibrillation — the pulse is gone") }
                    ]
                }));
            }
            Entry::Brady => {
                let to_arrest_rate = (64.0 - v.sbp) / pre_arrest_min;
                states.push(json!({
                    "id": "brady_unstable", "status": "critical", "rhythm": "sinus",
                    "dynamics": [
                        { "var": "sbp", "rate_per_min": to_arrest_rate.min(-0.5), "floor": 60.0 },
                        { "var": "hr", "rate_per_min": -1.5, "floor": 25.0 },
                        { "var": "spo2", "rate_per_min": -0.3, "floor": 88.0 }
                    ],
                    "bands": [],
                    "transitions": [
                        turn_edge,
                        { "to_state": "arrest_pea", "when": { "any": [ { "var": "sbp", "op": "lt", "value": 65.0 }, { "var": "hr", "op": "lt", "value": 30.0 } ] },
                          "do": arrest_entry_effects("rhythm: pulseless electrical activity — complexes march on with no pulse") }
                    ]
                }));
            }
            _ => {}
        }
        states.push(json!({
            "id": "converted", "status": "improving", "rhythm": "sinus",
            "dynamics": [
                { "var": "sbp", "rate_per_min": 5.0, "ceil": 118.0 },
                { "var": "spo2", "rate_per_min": 1.0, "ceil": 98.0 },
                { "var": "hr", "rate_per_min": if target_hr < 80.0 { 1.0 } else { -1.0 }, "floor": (target_hr - 10.0).max(60.0), "ceil": target_hr + 5.0 }
            ],
            "bands": [],
            "transitions": []
        }));
    }

    // ── the arrest core ───────────────────────────────────────────────────────────────
    let pea_needs_cause = mapped.get("reversible_causes").is_some() && matches!(e, Entry::Pea);
    let mut pea_rosc_all = vec![json!({ "var": "t_in_state", "op": "ge", "value": 120.0 })];
    pea_rosc_all.extend(cpr_and_adrenaline.clone());
    if pea_needs_cause {
        pea_rosc_all.push(json!({ "done": "tx_reversible_causes" }));
    }
    let mut post_shock_rosc_all = vec![json!({ "var": "t_in_state", "op": "ge", "value": 120.0 })];
    post_shock_rosc_all.extend(cpr_and_adrenaline.clone());
    post_shock_rosc_all.push(json!({ "var": "shocks", "op": "ge", "value": 2.0 }));
    post_shock_rosc_all.push(json!({ "any": [ { "var": "shocks", "op": "lt", "value": 3.0 }, { "flag": "amiodarone_given" } ] }));
    let mut asystole_rosc_all = vec![json!({ "var": "t_in_state", "op": "ge", "value": 240.0 })];
    asystole_rosc_all.extend(cpr_and_adrenaline.clone());

    states.push(json!({
        "id": "arrest_vf", "status": "arrest", "rhythm": "vf",
        "dynamics": perfusion_dynamics,
        "bands": [],
        "transitions": [
            dead,
            { "to_state": "post_shock_cpr", "when": { "rhythm": "sinus" },
              "do": [ { "delta": { "shocks": 1.0 } }, { "beat": "shock delivered — compressions resume at once; rhythm check in two minutes" } ] },
            { "to_state": "arrest_asystole", "when": { "var": "t_in_state", "op": "ge", "value": 360.0 },
              "do": [ { "beat": "rhythm: asystole — the fibrillation has run down to a flat line" } ] }
        ]
    }));
    states.push(json!({
        "id": "post_shock_cpr", "status": "arrest", "rhythm": "sinus",
        "dynamics": perfusion_dynamics,
        "bands": [],
        "transitions": [
            dead,
            { "to_state": "rosc", "when": { "all": post_shock_rosc_all }, "do": rosc_entry_effects() },
            { "to_state": "arrest_vf", "when": { "var": "t_in_state", "op": "ge", "value": 120.0 },
              "do": [ { "beat": "rhythm check — ventricular fibrillation again; charge" } ] }
        ]
    }));
    states.push(json!({
        "id": "arrest_pea", "status": "arrest", "rhythm": "pea",
        "dynamics": perfusion_dynamics,
        "bands": [],
        "transitions": [
            dead,
            { "to_state": "rosc", "when": { "all": pea_rosc_all }, "do": rosc_entry_effects() },
            { "to_state": "arrest_asystole", "when": { "var": "t_in_state", "op": "ge", "value": 480.0 },
              "do": [ { "beat": "rhythm: asystole — the complexes have gone" } ] }
        ]
    }));
    states.push(json!({
        "id": "arrest_asystole", "status": "arrest", "rhythm": "asystole",
        "dynamics": perfusion_dynamics,
        "bands": [],
        "transitions": [
            dead,
            { "to_state": "rosc", "when": { "all": asystole_rosc_all }, "do": rosc_entry_effects() }
        ]
    }));
    states.push(json!({
        "id": "rosc", "status": "improving", "rhythm": "sinus",
        "dynamics": [
            { "var": "sbp", "rate_per_min": 3.0, "ceil": 110.0 },
            { "var": "spo2", "rate_per_min": 2.0, "ceil": 97.0 },
            { "var": "hr", "rate_per_min": -2.0, "floor": 90.0 },
            { "var": "perfusion", "rate_per_min": 5.0, "ceil": 100.0 }
        ],
        "bands": [],
        "transitions": []
    }));

    let initial = match e {
        Entry::Vf => "arrest_vf",
        Entry::Pea => "arrest_pea",
        Entry::Asystole => "arrest_asystole",
        Entry::TachyStable => "tachy_stable",
        Entry::TachyUnstable => "tachy_unstable",
        Entry::Brady => "brady_unstable",
    };

    // ── triggers ──────────────────────────────────────────────────────────────────────
    let mut triggers: Vec<Value> = Vec::new();
    let mut trigger_harms: Vec<(String, String)> = Vec::new();
    let mut late: Vec<(String, f64)> = Vec::new();

    for (state, id) in [("rosc", "recovered"), ("converted", "recovered_converted")] {
        if states.iter().any(|s| s["id"] == state) {
            let ending: Vec<Value> = if state == "converted" && tachy && !icu_fixed {
                vec![json!({ "branch": [ { "if": { "flag": "cardioversion_given" }, "then": [ { "outcome": "win_icu" } ] } ], "else": [ { "outcome": "win_discharge" } ] })]
            } else {
                vec![json!({ "outcome": win })]
            };
            triggers.push(json!({
                "id": id, "once": true,
                "when": { "all": [ { "in_state": state }, { "var": "t_in_state", "op": "ge", "value": a.recovery_sec() } ] },
                "do": ending
            }));
        }
    }
    triggers.push(json!({
        "id": "wakes", "once": true,
        "when": { "all": [ in_any(&["rosc", "converted"]), { "var": "t_in_state", "op": "ge", "value": 240.0 } ] },
        "do": [ { "set": { "gcs": if with_pulse { 15.0 } else { 8.0 } } }, { "beat": if with_pulse { "awake, oriented, asking what happened" } else { "beginning to localise — sedated for the tube, the pupils react" } } ]
    }));

    if !with_pulse {
        if e == Entry::Vf {
            let by = mapped.timed.get("defibrillate").map(|t| t.named_sec.min(FIRST_SHOCK_BY_SEC)).unwrap_or(FIRST_SHOCK_BY_SEC);
            let text = format!("the first shock was not delivered by {}:{:02} — the window closed", (by as u32) / 60, (by as u32) % 60);
            triggers.push(json!({
                "id": "late_defibrillate", "once": true,
                "when": { "all": [ { "in_state": "arrest_vf" }, { "var": "t_elapsed", "op": "ge", "value": by }, { "var": "shocks", "op": "lt", "value": 1.0 } ] },
                "do": [ { "harm": text }, { "beat": "the window for the first shock has closed" } ]
            }));
            trigger_harms.push(("late_defibrillate".into(), text));
            late.push(("defibrillate".into(), by));
        }
        let by = ADRENALINE_WINDOW_SEC;
        let text = format!("adrenaline was not given by {}:{:02} — the window closed", (by as u32) / 60, (by as u32) % 60);
        triggers.push(json!({
            "id": "late_adrenaline_iv", "once": true,
            "when": { "all": [ in_any(&ARREST_STATES), { "var": "t_elapsed", "op": "ge", "value": by }, { "flag": "adrenaline_iv_given", "is": false } ] },
            "do": [ { "harm": text }, { "beat": "the window for the first adrenaline has closed" } ]
        }));
        trigger_harms.push(("late_adrenaline_iv".into(), text));
        late.push(("adrenaline_iv".into(), by));
    }
    // the shock the engine refuses is priced whichever route it came by
    trigger_harms.push(("shock_non_shockable".into(), NOT_SHOCKABLE.to_string()));

    let variables = json!({
        "perfusion": { "init": if with_pulse { 100.0 } else { 60.0 }, "min": 0.0, "max": 100.0 },
        "shocks": { "init": 0.0, "min": 0.0, "max": 30.0 }
    });

    let (v_hr, v_sbp, v_dbp, v_spo2, v_rr, v_gcs) = if with_pulse {
        (v.hr, v.sbp, v.dbp, v.spo2, v.rr, v.gcs)
    } else {
        (0.0, 0.0, 0.0, 0.0, 0.0, 3)
    };

    let label_of = |w: &str| match w {
        "win_icu" => "A pulse, a pressure and a plan — handed to intensive care",
        _ => "Converted and observed — home with a plan",
    };
    let mut outcomes: Vec<Value> = wins.iter().map(|w| json!({ "id": w, "kind": "win", "label": label_of(w) })).collect();
    outcomes.push(json!({ "id": "death_arrest", "kind": "death", "label": "The no-flow time ran out — the arrest could not be reversed" }));

    // the algorithm's tools the sheet pays for: compressions and adrenaline in every arrest, the
    // shock only where the rhythm at the door is shockable — a shock into PEA is the harm, not
    // the item
    let paid_rescue: Vec<&'static str> = match e {
        Entry::Vf => vec!["cpr", "defibrillate", "adrenaline_iv"],
        Entry::Pea | Entry::Asystole => vec!["cpr", "adrenaline_iv"],
        _ => Vec::new(),
    };

    // the debrief: what the algorithm expects, timed where it times it
    let mut expect: Vec<Value> = Vec::new();
    for p in mapped.present.iter().filter(|p| matches!(p.role.kind, Kind::Critical | Kind::Gate) || (p.role.kind == Kind::Rescue && paid_rescue.contains(&p.role.id))) {
        let mut ex = json!({ "id": p.tx_id(), "label": p.role.label });
        if let Some((_, by)) = late.iter().find(|(id, _)| id == p.role.id) {
            ex["within_sec"] = json!(by);
        }
        expect.push(ex);
    }

    let sce = json!({
        "_note": format!(
            "Compiled by vitals-casefactory from embla-cases {} under the {} archetype (ACLS family; entry {:?}). Provisional: shaped by a deterministic compiler on the ACLS algorithm, not clinically reviewed. Rhythm is the engine's per-state rhythm and is spoken in beats; a morphology for rhythms with a pulse needs an engine variable.",
            case.meta.id, a.id(), e
        ),
        "setting": "ED",
        "tick_seconds": 1.0,
        "vitals0": { "hr": v_hr, "sbp": v_sbp, "dbp": v_dbp, "spo2": v_spo2, "rr": v_rr, "temp": v.temp, "gcs": v_gcs },
        "variables": variables,
        "initial_state": initial,
        "states": states,
        "interventions": built.interventions,
        "triggers": triggers,
        "outcomes": outcomes,
        "debrief": {
            "expect": expect,
            "avoid": mapped.harmful().iter().map(|p| json!({ "id": p.tx_id(), "label": p.role.label, "why": p.role.harm })).collect::<Vec<_>>()
        }
    });

    Sim { sce, win, wins, paid_rescue, late, trigger_harms }
}

pub const NOT_SHOCKABLE: &str = "unsynchronised shock into a rhythm that is not shockable — compressions and adrenaline lost";
const SHOCK_PERFUSING: &str = "unsynchronised shock to a perfusing rhythm — it can drive a pulse into ventricular fibrillation";

/// The effects of the algorithm's own interventions: each one asks which state it is in, so a
/// shock in VF, a shock in PEA and a shock into a pulse do three different things.
pub fn effects(role_id: &str, a: Archetype, mapped: &Mapped) -> Option<Vec<Value>> {
    if !a.is_acls() {
        return None;
    }
    let in_arrest = in_any(&ARREST_STATES);
    let in_tachy = in_any(&TACHY_STATES);
    let in_brady = json!({ "in_state": "brady_unstable" });
    let refused = |text: &str| vec![json!({ "beat": text })];
    let v = match role_id {
        "cpr" => vec![json!({ "branch": [
            { "if": in_arrest, "then": [ { "flag": "cpr_active", "for_sec": CPR_WINDOW_SEC }, { "flag": "cpr_given" }, { "beat": "compressions — hard, fast, full recoil; the compressor changes at two minutes" } ] }
        ], "else": [ { "harm": "chest compressions on a patient with a pulse — the pulse was never checked" } ] })],
        "defibrillate" => vec![json!({ "branch": [
            { "if": { "in_state": "arrest_vf" }, "then": [ { "delta": { "shocks": 1.0 } }, { "flag": "defibrillate_given" }, { "beat": "200 J — the trace jolts; compressions resume at once" }, { "to_state": "post_shock_cpr" } ] },
            { "if": in_any(&["arrest_pea", "arrest_asystole", "post_shock_cpr"]), "then": [ { "harm": NOT_SHOCKABLE } ] }
        ], "else": [ { "harm": SHOCK_PERFUSING } ] })],
        "adrenaline_iv" => vec![json!({ "branch": [
            { "if": in_arrest, "then": [ { "flag": "adrenaline_active", "for_sec": ADRENALINE_WINDOW_SEC }, { "flag": "adrenaline_iv_given" }, { "beat": "adrenaline 1 mg — flushed; the clock for the next dose starts" } ] }
        ], "else": [ { "harm": "1 mg of adrenaline into a patient with a pulse — a hypertensive surge and a dangerous arrhythmia" }, { "delta": { "hr": 30.0 }, "cap": 200.0 } ] })],
        "amiodarone" => vec![json!({ "branch": [
            { "if": in_arrest, "then": [ { "flag": "amiodarone_given" }, { "beat": "amiodarone 300 mg — for the rhythm that keeps coming back" } ] },
            { "if": in_tachy, "then": [ { "flag": "rate_control_given" }, { "flag": "amiodarone_given" }, { "delta": { "hr": -15.0 }, "floor": 80.0 }, { "beat": "amiodarone infusing — the rate eases" } ] }
        ], "else": refused("amiodarone has no place here") })],
        "cardioversion" => {
            let target = converted_hr(a);
            let stable_then: Vec<Value> = if a == Archetype::AclsTachycardiaAf {
                vec![json!({ "branch": [
                    { "if": { "flag": "anticoagulation_given" }, "then": [ { "flag": "cardioversion_given" }, { "beat": "synchronised shock for a stable rhythm — it converts; the drugs would have, without a shock" }, { "set": { "hr": target } }, { "to_state": "converted" } ] }
                ], "else": [ { "harm": "cardioversion of atrial fibrillation without anticoagulation — a clot leaves the atrium" }, { "flag": "cardioversion_given" }, { "set": { "hr": target } }, { "to_state": "converted" } ] })]
            } else {
                vec![json!({ "flag": "cardioversion_given" }), json!({ "beat": "synchronised shock for a stable rhythm — it converts; adenosine would have, without a shock" }), json!({ "set": { "hr": target } }), json!({ "to_state": "converted" })]
            };
            vec![json!({ "branch": [
                { "if": { "in_state": "tachy_unstable" }, "then": [ { "flag": "cardioversion_given" }, { "beat": "synchronised 100 J — a pause, then sinus rhythm and a pressure" }, { "set": { "hr": target } }, { "to_state": "converted" } ] },
                { "if": { "in_state": "tachy_stable" }, "then": stable_then },
                { "if": in_brady, "then": [ { "harm": "a synchronised shock to a bradycardia — there is nothing to convert" } ] },
                { "if": in_arrest, "then": [ { "harm": "sync mode in an arrest — the machine waits for a complex that never comes; compressions lost" } ] }
            ], "else": refused("nothing here to cardiovert") })]
        }
        "vagal" => vec![json!({ "branch": [
            { "if": in_tachy, "then": [ { "flag": "vagal_given" }, { "beat": "a modified Valsalva — legs up, bear down; the monitor is watched" } ] }
        ], "else": refused("a vagal manoeuvre changes nothing here") })],
        "adenosine" if mapped.get("adenosine").is_some_and(|p| p.role.kind != Kind::Harmful) => vec![json!({ "branch": [
            { "if": in_tachy, "then": [ { "flag": "adenosine_given" }, { "beat": "adenosine 6 mg, rapid flush — a pause on the monitor, then the rhythm breaks" } ] }
        ], "else": refused("adenosine has no place in an arrest") })],
        "rate_control" => vec![json!({ "branch": [
            { "if": in_tachy, "then": [ { "flag": "rate_control_given" }, { "delta": { "hr": -20.0 }, "floor": 80.0 }, { "beat": "rate control in — the ventricular response slows and the pressure holds" } ] }
        ], "else": refused("a rate-slowing drug with no rapid rhythm to slow") })],
        "atropine" => vec![json!({ "branch": [
            { "if": in_brady, "then": [ { "flag": "atropine_given" }, { "delta": { "hr": 15.0 }, "cap": 80.0 }, { "beat": "atropine 1 mg — the rate lifts" } ] },
            { "if": in_arrest, "then": refused("atropine is no longer in the arrest algorithm") }
        ], "else": refused("atropine with nothing slow to speed up") })],
        "pacing" => vec![json!({ "branch": [
            { "if": in_brady, "then": [ { "flag": "pacing_given" }, { "set": { "hr": 70.0 } }, { "beat": "pads on, capture at 70 — the pressure follows the rate" } ] },
            { "if": { "in_state": "arrest_asystole" }, "then": refused("pacing in asystole is not recommended") }
        ], "else": refused("pads on standby") })],
        "chronotrope" => vec![json!({ "branch": [
            { "if": in_brady, "then": [ { "flag": "chronotrope_given" }, { "delta": { "hr": 10.0 }, "cap": 80.0 }, { "beat": "the infusion runs — the rate is held while the pads stand by" } ] }
        ], "else": refused("a chronotrope with nothing slow to drive") })],
        _ => return None,
    };
    Some(v)
}

/// The algorithm's own order of events for a case that begins without a pulse — the golden
/// path the validator replays and the rubric marks. `None` for entries with a pulse, whose
/// critical roles follow the generic order.
pub fn golden_prefix(case: &Case, a: Archetype, v: &Vitals0, mapped: &Mapped) -> Option<Vec<(f64, String)>> {
    if a != Archetype::AclsCardiacArrest {
        return None;
    }
    let has = |id: &str| mapped.get(id).is_some();
    let mut p: Vec<(f64, String)> = Vec::new();
    match entry(case, a, v) {
        Entry::Vf => {
            p.push((10.0, "tx_cpr".into()));
            p.push((20.0, "tx_defibrillate".into()));
            if has("airway") { p.push((30.0, "tx_airway".into())); }
            if has("reversible_causes") { p.push((60.0, "tx_reversible_causes".into())); }
            // rhythm check at 140: VF again — second shock, then adrenaline as the algorithm has it
            p.push((150.0, "tx_cpr".into()));
            p.push((155.0, "tx_defibrillate".into()));
            p.push((160.0, "tx_adrenaline_iv".into()));
        }
        Entry::Pea => {
            p.push((10.0, "tx_cpr".into()));
            p.push((20.0, "tx_adrenaline_iv".into()));
            if has("airway") { p.push((30.0, "tx_airway".into())); }
            if has("reversible_causes") { p.push((40.0, "tx_reversible_causes".into())); }
        }
        Entry::Asystole => {
            p.push((10.0, "tx_cpr".into()));
            p.push((20.0, "tx_adrenaline_iv".into()));
            if has("airway") { p.push((30.0, "tx_airway".into())); }
            p.push((130.0, "tx_cpr".into()));
            p.push((200.0, "tx_adrenaline_iv".into()));
        }
        _ => return None,
    }
    Some(p)
}
