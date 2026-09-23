//! A compiled pack is a scenario the engine parses, that dies untreated and lives when treated
//! in the order the case's own plan gives — proven by replaying it, not by reading it.

mod common;

use vitals_casefactory::{compile, Source};
use vitals_replay::{replay, Step};
use vitals_sce::Sce;

const SYNTHETIC: &str = include_str!("fixtures/synthetic-septic-shock.json");

fn src() -> Source {
    Source::of("embla-cases", "test", SYNTHETIC)
}

#[test]
fn the_synthetic_case_compiles_under_septic_shock_with_the_plan_as_interventions() {
    let pack = compile(SYNTHETIC, src()).expect("compiles");
    assert_eq!(pack.case_id, "synthetic-septic-shock-test");
    assert_eq!(pack.archetype, "septic_shock");
    assert_eq!(pack.difficulty, "intern");
    assert!(pack.provisional);
    assert!(!pack.endemic, "not tagged endemic");
    assert_eq!(pack.source.repo, "embla-cases");
    assert_eq!(pack.source.sha256.len(), 64);

    let sce_json = pack.sce.to_string();
    let sce = Sce::from_json(&sce_json).expect("the engine parses it");
    assert!(sce.validate().is_empty(), "{:?}", sce.validate());
    assert_eq!(sce.vitals0.sbp, 82.0);
    assert_eq!(sce.vitals0.gcs, 14);

    let ids: Vec<&str> = sce.interventions.iter().map(|i| i.id.as_str()).collect();
    for want in ["tx_fluids", "tx_antibiotics", "tx_vasopressor", "tx_oxygen", "tx_admit", "tx_nsaid", "dx_septic_shock", "ix_lactate_and_blood_gas", "ask_dysuria"] {
        assert!(ids.contains(&want), "missing {want} in {ids:?}");
    }
    // Every plan step is accounted for: mapped to interventions, or listed as unmapped.
    assert_eq!(pack.management.len(), 7);
    assert!(pack.management[0].interventions.contains(&"tx_antibiotics".to_string()));
    assert!(pack.management[1].interventions.contains(&"tx_fluids".to_string()));
}

#[test]
fn untreated_the_patient_dies_inside_the_archetypes_bound() {
    let pack = compile(SYNTHETIC, src()).unwrap();
    let sce_json = pack.sce.to_string();
    let bound = 24 * 60;
    let tape: Vec<Step> = (0..bound).map(|_| Step::Tick(1.0)).collect();
    let r = replay(&sce_json, &tape).unwrap();
    assert_eq!(r.outcome.as_deref(), Some("DeathArrest"));
    assert!(pack.replay.untreated_death_sec > 300.0, "too fast: {}", pack.replay.untreated_death_sec);
    assert!(pack.replay.untreated_death_sec <= bound as f64);
}

#[test]
fn the_management_path_the_pack_records_wins() {
    let pack = compile(SYNTHETIC, src()).unwrap();
    let sce_json = pack.sce.to_string();
    let mut tape = Vec::new();
    let mut t = 0.0;
    for step in &pack.replay.win_path {
        while t < step.t_sec {
            tape.push(Step::Tick(1.0));
            t += 1.0;
        }
        tape.push(Step::Act { text: step.id.clone(), id: step.id.clone() });
    }
    for _ in 0..(40 * 60) {
        tape.push(Step::Tick(1.0));
    }
    let r = replay(&sce_json, &tape).unwrap();
    assert_eq!(r.outcome.as_deref(), Some("WinIcu"), "beats: {:?}", r.beats);
    assert!(r.harm_events.is_empty(), "the golden path hurts nobody: {:?}", r.harm_events);
    assert!(pack.replay.win_sec > 0.0);
}

#[test]
fn the_voice_carries_the_patients_words_verbatim_keyed_by_the_ask() {
    let pack = compile(SYNTHETIC, src()).unwrap();
    let v = pack.voice.get("ask_dysuria").expect("the dysuria line is an ask");
    assert_eq!(v.words, "It burns when I pass water.");
    assert!(v.present);
    assert_eq!(v.reveal, "on_ask");
}

#[test]
fn a_case_no_archetype_fits_is_refused_not_written() {
    let mut case: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    case["exam_findings"][0]["value"] = serde_json::json!("128/82 mmHg");
    let s = case.to_string();
    let err = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_err();
    assert_eq!(err.case_id, "synthetic-septic-shock-test");
    assert!(err.reason.contains("not forced"), "{}", err.reason);
}

// ── the diagnosis keeps its own name ──────────────────────────────────────────────────────────
// Found on production, 23 Sep 2026: `ddx-pneumonia-1-en` and `-4-en` ship with a diagnosis
// matcher of `["dx_pneumonia"]` and nothing else — ten marks reachable by no word in any language.
// The pruning pass drops any single-word keyword that more than one intervention carries, and
// "pneumonia" was also in `ask_pneumonia_history`, so the diagnosis lost its only word.
//
// The rule is not "the diagnosis keeps the word": the engine resolves a learner's text to the
// **first** intervention in declaration order that matches, so a diagnosis that kept "pneumonia"
// behind an ask that also carried it would lose every time, with the patient answering a history
// question instead of the sheet recording a diagnosis — a worse zero than silence. So the test
// asserts the outcome: typing the disease's bare name resolves to the diagnosis.
#[test]
fn typing_the_bare_name_of_the_disease_names_the_diagnosis_even_when_a_history_question_shares_the_word() {
    use vitals_sce::runtime::SceState;
    let mut case: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    case["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": "Pneumonia", "aliases": [] });
    case["symptom_script"].as_array_mut().unwrap().push(serde_json::json!({
        "finding": { "display": "Pneumonia history" }, "present": true, "reveal": "asked",
        "patient_words": "I had pneumonia once, years ago."
    }));
    let pack = compile(&case.to_string(), src()).expect("compiles");
    let sce = Sce::from_json(&pack.sce.to_string()).expect("the engine parses it");
    let st = SceState::new(sce);
    let hit = st.resolve("pneumonia");
    assert!(
        hit.as_deref().is_some_and(|id| id.starts_with("dx_")),
        "typing the disease's name should name the diagnosis, not fire {hit:?}"
    );
    // The trade, stated: the history question that shares the disease's word is still on the
    // case under its own id — the ward's ask path (`/api/say?q=<id>`) finds it by that id, never
    // through this matcher — and typed prose carrying the word now names the diagnosis, because
    // the engine takes the first match and the diagnosis comes first. A learner who types the
    // disease is naming it; the question keeps its chip.
    let sce2 = Sce::from_json(&pack.sce.to_string()).unwrap();
    assert!(sce2.interventions.iter().any(|i| i.id == "ask_pneumonia_history"), "the history question is still on the case under its id");
    assert!(st.resolve("pneumonia history").as_deref().is_some_and(|id| id.starts_with("dx_")), "typed prose with the disease's word names the diagnosis — the trade this test records");
    // But an *order* that mentions the disease is an order. The diagnosis is the first
    // intervention any text with the disease's word can hit, so without a guard "blood gas for
    // pneumonia" would record a diagnosis the learner never made and send no blood gas — a
    // premature commitment invisible on the receipt. The guard is derived from the case's own
    // other keywords, not from medical knowledge.
    let ix = st.resolve("lactate for pneumonia");
    assert!(ix.as_deref().is_some_and(|id| id.starts_with("ix_")), "an order that mentions the disease resolves to the order, not the diagnosis: {ix:?}");
    let tx = st.resolve("antibiotics for pneumonia");
    assert!(tx.as_deref().is_some_and(|id| id.starts_with("tx_")), "a treatment that mentions the disease resolves to the treatment: {tx:?}");
}
