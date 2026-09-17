//! The ACLS-shaped family: a case that names a rhythm or an arrest compiles into states named by
//! rhythm, moved by the algorithm's steps and by the two-minute clock.

mod common;

use vitals_casefactory::{compile, Source};
use vitals_replay::{replay, Step};
use vitals_sce::{Sce, SceState};

const VF: &str = include_str!("fixtures/synthetic-vf-arrest.json");

fn vf_pack() -> vitals_casefactory::Pack {
    compile(VF, Source::of("embla-cases", "test", VF)).unwrap_or_else(|e| panic!("{}", e.reason))
}

#[test]
fn a_vf_arrest_compiles_with_states_named_by_rhythm_and_dies_untreated_inside_the_bound() {
    let pack = vf_pack();
    assert_eq!(pack.archetype, "acls_cardiac_arrest");
    let sce = Sce::from_json(&pack.sce.to_string()).unwrap();
    assert!(sce.validate().is_empty(), "{:?}", sce.validate());
    let ids: Vec<&str> = sce.states.iter().map(|s| s.id.as_str()).collect();
    for want in ["arrest_vf", "post_shock_cpr", "arrest_pea", "arrest_asystole", "rosc"] {
        assert!(ids.contains(&want), "missing state {want} in {ids:?}");
    }
    assert_eq!(sce.initial_state, "arrest_vf");
    // the engine's own rhythm on the state, so the monitor draws VF and the shock button knows
    let vf = sce.states.iter().find(|s| s.id == "arrest_vf").unwrap();
    assert_eq!(vf.rhythm.as_deref(), Some("vf"));
    assert_eq!(vf.status.as_deref(), Some("arrest"));
    let d = pack.replay.untreated_death_sec;
    assert!((120.0..=480.0).contains(&d), "untreated death at {d} s");
}

#[test]
fn the_algorithm_reaches_rosc_and_the_win_cpr_shock_adrenaline_two_minute_check() {
    let pack = vf_pack();
    assert_eq!(pack.replay.win_outcome, "win_icu");
    let ids: Vec<&str> = pack.replay.win_path.iter().map(|p| p.id.as_str()).collect();
    for want in ["tx_cpr", "tx_defibrillate", "tx_adrenaline_iv", "tx_airway"] {
        assert!(ids.contains(&want), "path lacks {want}: {ids:?}");
    }
    // the shock comes before the adrenaline, as the algorithm has it
    let at = |id: &str| pack.replay.win_path.iter().find(|p| p.id == id).map(|p| p.t_sec).unwrap();
    assert!(at("tx_defibrillate") < at("tx_adrenaline_iv"));
    // ROSC is declared at a rhythm check, so the win is at least one two-minute cycle after the shock
    assert!(pack.replay.win_sec >= at("tx_defibrillate") + 120.0);
}

#[test]
fn the_kit_shock_button_converts_vf_the_same_way_the_typed_order_does() {
    let pack = vf_pack();
    let sce_json = pack.sce.to_string();
    // typed
    let mut typed = vec![Step::Tick(10.0), Step::Act { text: "cpr".into(), id: "tx_cpr".into() }, Step::Act { text: "shock".into(), id: "tx_defibrillate".into() }];
    typed.push(Step::Tick(1.0));
    let (st, _) = vitals_replay::resume(&sce_json, &typed).unwrap();
    let typed_state = st.events().iter().filter(|e| e.kind == "beat").count();
    let _ = typed_state;
    // the button: Step::Shock goes straight at the engine, which converts VF to sinus and the
    // scenario's own edge takes it from there
    let button = vec![Step::Tick(10.0), Step::Act { text: "cpr".into(), id: "tx_cpr".into() }, Step::Shock(200.0), Step::Tick(1.0)];
    let (st2, r) = vitals_replay::resume(&sce_json, &button).unwrap();
    assert!(r.harm_events.is_empty(), "a shock into VF is not harm: {:?}", r.harm_events);
    assert_eq!(st2.vitals.rhythm, vitals_sce::runtime::Rhythm::Sinus);
    assert!(st.vitals.rhythm == vitals_sce::runtime::Rhythm::Sinus, "the typed shock leaves the same rhythm");
}

#[test]
fn shocking_a_non_shockable_rhythm_is_harm_and_the_rubric_prices_it() {
    // typed, into PEA: the same sentence the engine's own button writes, so one needle prices both
    let mut v: serde_json::Value = serde_json::from_str(VF).unwrap();
    v["meta"]["id"] = serde_json::json!("synthetic-pea-arrest-test");
    v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": "Cardiac arrest — pulseless electrical activity", "aliases": ["PEA arrest", "cardiac arrest"] });
    let s = v.to_string();
    let pack = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_or_else(|e| panic!("{}", e.reason));
    let sce = Sce::from_json(&pack.sce.to_string()).unwrap();
    let mut st = SceState::new(sce);
    st.tick(5.0);
    st.apply_id("tx_defibrillate");
    assert!(st.harm_events.iter().any(|h| h.contains("not shockable")), "{:?}", st.harm_events);
    let needles: Vec<String> = pack.rubric["items"].as_array().unwrap().iter()
        .filter(|i| i["type"] == "no_harm").map(|i| i["needle"].as_str().unwrap().to_string()).collect();
    assert!(needles.iter().any(|n| n.contains("not shockable")), "{needles:?}");
    // and untreated, a VF that nobody shocks dies of no-flow before it can run down to a flat line
    let vf = vf_pack();
    assert!(vf.replay.untreated_death_sec < 360.0, "{}", vf.replay.untreated_death_sec);
}

#[test]
fn the_first_shock_and_the_adrenaline_are_timed_items() {
    let pack = vf_pack();
    let timed: Vec<(String, f64)> = pack.rubric["items"].as_array().unwrap().iter()
        .filter(|i| i["type"] == "action_by")
        .map(|i| (i["needle"].as_str().unwrap().to_string(), i["by_sec"].as_f64().unwrap()))
        .collect();
    assert!(timed.iter().any(|(n, by)| n == "tx_defibrillate" && *by <= 120.0), "{timed:?}");
    assert!(timed.iter().any(|(n, _)| n == "tx_adrenaline_iv"), "{timed:?}");
}

#[test]
fn a_synthetic_pea_presentation_compiles_and_shocks_are_harm_from_the_first_second() {
    let mut v: serde_json::Value = serde_json::from_str(VF).unwrap();
    v["meta"]["id"] = serde_json::json!("synthetic-pea-arrest-test");
    v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": "Cardiac arrest — pulseless electrical activity", "aliases": ["PEA arrest", "cardiac arrest"] });
    v["investigations"][0]["result"]["value"] = serde_json::json!("Organised complexes at 40/min, no pulse — PEA");
    let s = v.to_string();
    let pack = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_or_else(|e| panic!("{}", e.reason));
    let sce = Sce::from_json(&pack.sce.to_string()).unwrap();
    assert_eq!(sce.initial_state, "arrest_pea");
    let r = replay(&pack.sce.to_string(), &[Step::Tick(5.0), Step::Shock(200.0)]).unwrap();
    assert!(r.harm_events.iter().any(|h| h.contains("not shockable")), "{:?}", r.harm_events);
}

#[test]
fn the_library_rhythm_cases_fit_the_tachycardia_shapes_below_the_language_gate() {
    // The Thai library cases are refused by the language gate before anything else is read;
    // below it, the archetype layer still reads them right — so the day a translation step
    // exists they compile under the shapes they fit, and the controlled ones stay refused.
    use vitals_casefactory::archetype::Archetype;
    use vitals_casefactory::embla::parse_case;
    let Some(dir) = common::embla_dir() else { return common::skip("embla-cases not present"); };
    let mut failures = Vec::new();
    // two of the four atrial fibrillations arrive rate-controlled (88 and 92 a minute, a normal
    // pressure): not a deterioration, and never another archetype's patient either
    for id in ["ddx-atrial-fibrillation-1", "ddx-atrial-fibrillation-3"] {
        let Some(json) = common::library_case(&dir, id) else { continue };
        let case = parse_case(&json).unwrap();
        let v0 = case.vitals0().unwrap();
        match Archetype::detect(&case, &v0) {
            Ok(a) => failures.push(format!("{id}: fits {} but is a controlled rhythm", a.id())),
            Err(e) => {
                if !e.contains("controlled rhythm") { failures.push(format!("{id}: {e}")); }
            }
        }
    }
    for (id, want) in [
        ("ddx-psvt-1", Archetype::AclsTachycardiaSvt), ("ddx-psvt-2", Archetype::AclsTachycardiaSvt),
        ("ddx-psvt-3", Archetype::AclsTachycardiaSvt), ("ddx-psvt-4", Archetype::AclsTachycardiaSvt),
        ("ddx-atrial-fibrillation-2", Archetype::AclsTachycardiaAf), ("ddx-atrial-fibrillation-4", Archetype::AclsTachycardiaAf),
    ] {
        let Some(json) = common::library_case(&dir, id) else { failures.push(format!("{id}: not in library")); continue };
        let case = parse_case(&json).unwrap();
        let v0 = case.vitals0().unwrap();
        match Archetype::detect(&case, &v0) {
            Ok(a) if a == want => {}
            Ok(a) => failures.push(format!("{id}: {} not {}", a.id(), want.id())),
            Err(e) => failures.push(format!("{id}: {e}")),
        }
        let err = compile(&json, Source::of("embla-cases", "worktree", &json)).unwrap_err();
        if err.reason != "language: th — no translation step yet" {
            failures.push(format!("{id}: refused for {:?}, not for its language", err.reason));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_generic_arrhythmia_word_in_a_red_flag_does_not_make_a_rhythm_case() {
    // myocarditis with "dangerous arrhythmia" in the red flags is not an ACLS case
    let mut v: serde_json::Value = serde_json::from_str(VF).unwrap();
    v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": "Myocarditis", "aliases": [] });
    v["meta"]["search_tags"] = serde_json::json!([]);
    v["meta"]["title"] = serde_json::json!("Chest pain and breathlessness");
    v["hidden"]["red_flags"] = serde_json::json!(["dangerous arrhythmia such as VT/VF may occur"]);
    v["exam_findings"][0]["value"] = serde_json::json!("110/70 mmHg");
    v["exam_findings"][1]["value"] = serde_json::json!("104/min");
    v["exam_findings"][2]["value"] = serde_json::json!("20/min");
    v["exam_findings"][3]["value"] = serde_json::json!("96%");
    let s = v.to_string();
    let err = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_err();
    assert!(!err.reason.contains("acls"), "{}", err.reason);
}

#[test]
fn a_symptomatic_bradycardia_is_turned_by_atropine_then_pacing_and_arrests_in_pea_untreated() {
    let mut v: serde_json::Value = serde_json::from_str(VF).unwrap();
    v["meta"]["id"] = serde_json::json!("synthetic-brady-test");
    v["meta"]["title"] = serde_json::json!("Synthetic test case: dizzy and grey with a pulse of 36");
    v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": "Symptomatic bradycardia — complete heart block", "aliases": ["complete heart block", "third-degree AV block"] });
    v["meta"]["search_tags"] = serde_json::json!(["synthetic", "bradycardia", "test"]);
    v["exam_findings"][0]["value"] = serde_json::json!("78/50 mmHg");
    v["exam_findings"][1]["value"] = serde_json::json!("36/min, regular");
    v["exam_findings"][2]["value"] = serde_json::json!("18/min");
    v["exam_findings"][3]["value"] = serde_json::json!("95% on room air");
    v["exam_findings"][4]["value"] = serde_json::json!("Pale, sweaty, GCS 15");
    v["hidden"]["red_flags"] = serde_json::json!(["Hypotension with a rate of 36 = unstable bradycardia — atropine now, pads on"]);
    v["hidden"]["management_plan"] = serde_json::json!([
        "Atropine 1 mg IV, repeated every 3-5 minutes to 3 mg",
        "Transcutaneous pacing if atropine fails; sedation for the pads",
        "Dopamine or adrenaline infusion while pacing is prepared",
        "Admit to a monitored bed for a permanent pacemaker"
    ]);
    let s = v.to_string();
    let pack = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_or_else(|e| panic!("{}", e.reason));
    assert_eq!(pack.archetype, "acls_bradycardia");
    let sce = Sce::from_json(&pack.sce.to_string()).unwrap();
    assert_eq!(sce.initial_state, "brady_unstable");
    // untreated it arrests in PEA, then dies of no-flow
    let mut st = SceState::new(sce);
    let mut saw_pea = false;
    for _ in 0..1500 {
        st.tick(1.0);
        if st.vitals.rhythm == vitals_sce::runtime::Rhythm::Pea { saw_pea = true; }
        if st.outcome().is_some() { break; }
    }
    assert!(saw_pea, "the bradycardia degenerates to PEA before death");
    assert_eq!(st.outcome_id(), Some("death_arrest"));
    let ids: Vec<&str> = pack.replay.win_path.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.contains(&"tx_atropine") && ids.contains(&"tx_pacing"), "{ids:?}");
}
