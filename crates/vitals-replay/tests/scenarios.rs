//! Every scenario must parse, and every scenario must be winnable and losable.
//!
//! A mock that cannot be lost teaches nothing and proves nothing; a mock that cannot be won is a
//! bug wearing a story. These tapes are deliberately short and blunt — a liveness check on the
//! automaton, not a demonstration of good medicine.

use std::path::PathBuf;
use vitals_replay::{replay, Step};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tick(s: f64) -> Step { Step::Tick(s) }
fn act(s: &str) -> Step { Step::Do(s.into()) }

fn run(file: &str, tape: &[Step]) -> (Option<String>, usize) {
    let json = std::fs::read_to_string(dir().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
    let r = replay(&json, tape).unwrap_or_else(|e| panic!("{file}: {e}"));
    (r.outcome, r.harm_events.len())
}

/// Long enough for any of these scenarios to reach a terminal state if it is going to.
fn drift(tape: &mut Vec<Step>) {
    for _ in 0..6 {
        tape.push(tick(300.0));
    }
}

fn play(file: &str, actions: &[&str]) -> String {
    let mut tape = vec![tick(20.0)];
    for a in actions {
        tape.push(act(a));
        tape.push(tick(30.0));
    }
    drift(&mut tape);
    run(file, &tape).0.unwrap_or_else(|| panic!("{file}: reached no terminal state"))
}

fn win(file: &str, actions: &[&str]) {
    let o = play(file, actions);
    assert!(o.starts_with("Win"), "{file}: playing well ended in {o}");
}

fn lose(file: &str, actions: &[&str]) {
    let o = play(file, actions);
    assert!(o.starts_with("Death"), "{file}: playing badly ended in {o}");
}

#[test]
fn ep1_anaphylaxis() {
    win("conformance/sce-anaphylaxis-ep1.json", &["adrenaline im", "oxygen", "supine", "admit"]);
    lose("conformance/sce-anaphylaxis-ep1.json", &["chlorpheniramine", "hydrocortisone"]);
}

#[test]
fn ep2_stemi() {
    win("demo/scenarios/ep2-stemi.json", &["ecg", "aspirin", "activate the cath lab"]);
    lose("demo/scenarios/ep2-stemi.json", &["wait for the troponin"]);
}

#[test]
fn ep3_epiglottitis() {
    win("demo/scenarios/ep3-epiglottitis.json",
        &["keep him calm", "blow-by oxygen", "call ent and anaesthesia", "secure the airway", "ceftriaxone"]);
    lose("demo/scenarios/ep3-epiglottitis.json", &["look in the throat", "iv access", "separate the child"]);
}

#[test]
fn ep4_pulmonary_embolism() {
    win("demo/scenarios/ep4-pulmonary-embolism.json", &["wells score", "ctpa", "heparin"]);
    lose("demo/scenarios/ep4-pulmonary-embolism.json", &["ecg", "reassure and discharge"]);
}

#[test]
fn ep5_finale() {
    win("demo/scenarios/ep5-the-night-the-stars-fell.json",
        &["triage", "tourniquet", "needle decompression", "transfuse", "damage control"]);
    lose("demo/scenarios/ep5-the-night-the-stars-fell.json", &["two litres of saline"]);
}

/// The counter-intuitive lesson of EP3, asserted rather than described: reflex actions that would
/// be right anywhere else are what kill this child.
#[test]
fn ep3_punishes_the_reflex_actions() {
    let (_, harm) = run("demo/scenarios/ep3-epiglottitis.json",
        &[tick(10.0), act("look in the throat"), tick(10.0), act("iv access"), tick(60.0)]);
    assert!(harm >= 2, "examining the throat and cannulating should both be recorded as harm, got {harm}");
}
