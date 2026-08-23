//! Every order has to leave a timestamp behind.
//!
//! `apply()` matched an intervention, ran its effects, and put its id in a set — but recorded
//! nothing on the timeline. So the chart could say a mask went on and at what flow, and could not
//! say that adrenaline was ever given, let alone when. A debrief cannot be written from a record
//! that does not contain the orders.
//!
//! This is what makes a debrief recomputable: the time and the ordering are facts the automaton
//! derives from the tape, so a verifier reaches the same ones.

use std::path::PathBuf;
use vitals_sce::{Sce, SceState};

fn ep1() -> Sce {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/sce-anaphylaxis-ep1.json");
    Sce::from_json(&std::fs::read_to_string(&p).unwrap()).expect("parse")
}

/// The id, not the words the learner typed. "give her the pen" and "adrenaline im" are the same
/// order and must land on the timeline as the same fact.
#[test]
fn an_order_is_recorded_by_intervention_id() {
    let mut st = SceState::new(ep1());
    st.tick(90.0);
    st.apply("adrenaline im");
    let acted: Vec<_> = st.events().iter().filter(|e| e.kind == "action").collect();
    assert_eq!(acted.len(), 1, "the order left no record: {:?}", st.events());
    assert_eq!(acted[0].text, "adrenaline_im", "recorded as `{}`", acted[0].text);
    assert!((acted[0].t_sec - 90.0).abs() < 1e-6, "stamped at {}", acted[0].t_sec);
}

/// Text that matches nothing must not invent an order.
#[test]
fn something_that_matches_no_intervention_records_nothing() {
    let mut st = SceState::new(ep1());
    st.apply("tell her a joke");
    assert!(st.events().iter().all(|e| e.kind != "action"));
}

/// Repeating an order is a fact about the run — a second dose at a different minute is not the
/// same as one dose, and a debrief has to be able to see both.
#[test]
fn a_repeated_order_is_recorded_twice() {
    let mut st = SceState::new(ep1());
    st.apply("adrenaline im");
    st.tick(300.0);
    st.apply("adrenaline im");
    let acted: Vec<_> = st.events().iter().filter(|e| e.kind == "action").collect();
    assert_eq!(acted.len(), 2);
    assert!((acted[1].t_sec - 300.0).abs() < 1e-6);
}

/// The order goes on the timeline before the harm it caused, because that is the order they
/// happened in and the debrief attributes harm to the order that preceded it.
#[test]
fn a_harmful_order_is_recorded_before_its_harm() {
    let mut st = SceState::new(ep1());
    st.tick(60.0);
    st.apply("let her stand up");
    let kinds: Vec<&str> = st.events().iter().map(|e| e.kind.as_str()).collect();
    let a = kinds.iter().position(|k| *k == "action").expect("no order recorded");
    let h = kinds.iter().position(|k| *k == "harm").expect("standing her up is harm");
    assert!(a < h, "harm recorded before the order that caused it: {kinds:?}");
}
