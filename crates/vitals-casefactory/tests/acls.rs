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
    let pack = vf_pack();
    let sce_json = pack.sce.to_string();
    let sce = Sce::from_json(&sce_json).unwrap();
    // drive the machine into PEA by hand: the scenario's own path there is the untreated one,
    // so instead start from the pea state's declared rhythm by applying the case's typed shock
    // while the rhythm is not shockable
    let mut st = SceState::new(sce);
    // VF degenerates to asystole untreated; walk there
    let mut t = 0.0;
    while st.vitals.rhythm != vitals_sce::runtime::Rhythm::Asystole && t < 900.0 && st.outcome().is_none() {
        st.tick(1.0);
        t += 1.0;
    }
    assert_eq!(st.vitals.rhythm, vitals_sce::runtime::Rhythm::Asystole, "untreated VF reaches asystole before death (t={t})");
    st.apply_id("tx_defibrillate");
    assert!(st.harm_events.iter().any(|h| h.contains("not shockable")), "{:?}", st.harm_events);
    let needles: Vec<String> = pack.rubric["items"].as_array().unwrap().iter()
        .filter(|i| i["type"] == "no_harm").map(|i| i["needle"].as_str().unwrap().to_string()).collect();
    assert!(needles.iter().any(|n| n.contains("not shockable")), "{needles:?}");
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
fn the_eight_library_rhythm_cases_compile_under_the_tachycardia_archetypes() {
    let Some(dir) = common::embla_dir() else { return common::skip("embla-cases not present"); };
    let mut failures = Vec::new();
    for (id, want) in [
        ("ddx-psvt-1", "acls_tachycardia_svt"), ("ddx-psvt-2", "acls_tachycardia_svt"),
        ("ddx-psvt-3", "acls_tachycardia_svt"), ("ddx-psvt-4", "acls_tachycardia_svt"),
        ("ddx-atrial-fibrillation-1", "acls_tachycardia_af"), ("ddx-atrial-fibrillation-2", "acls_tachycardia_af"),
        ("ddx-atrial-fibrillation-3", "acls_tachycardia_af"), ("ddx-atrial-fibrillation-4", "acls_tachycardia_af"),
    ] {
        let Some(json) = common::library_case(&dir, id) else { failures.push(format!("{id}: not in library")); continue };
        match compile(&json, Source::of("embla-cases", "worktree", &json)) {
            Ok(p) => {
                if p.archetype != want { failures.push(format!("{id}: {} not {want}", p.archetype)); }
                eprintln!("{id}: {} dies untreated {} s, wins {} at {} s", p.archetype, p.replay.untreated_death_sec, p.replay.win_outcome, p.replay.win_sec);
            }
            Err(e) => failures.push(format!("{id}: {}", e.reason)),
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
