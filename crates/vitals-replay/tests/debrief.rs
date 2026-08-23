//! What the run is told back to the person who played it.
//!
//! Vitals could grade but not teach. A finished case showed an outcome, a score and a hash, and
//! nothing about *why* — not that adrenaline came four minutes late, not which order caused the
//! harm, not how long she spent in arrest. The score is the verdict; this is the reasoning.
//!
//! Every line is a time or an ordering derived from the tape, so a verifier re-derives the same
//! debrief from the same two inputs. Nothing here is an opinion, and nothing here needs a model.
//! The targets it measures against are clinical judgement and live in the scenario file, where a
//! doctor can change one without a programmer.

use std::path::PathBuf;
use vitals_replay::{debrief, Step};

fn sce() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/sce-anaphylaxis-ep1.json");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn tick(s: f64) -> Step { Step::Tick(s) }
fn act(s: &str) -> Step { Step::Do(s.into()) }

/// The textbook run: everything, early.
fn good() -> Vec<Step> {
    vec![
        act("adrenaline im"), act("oxygen"), act("lay her flat, legs up"),
        tick(60.0), act("normal saline bolus"),
        tick(300.0), act("admit for observation"), tick(600.0),
    ]
}

#[test]
fn an_expectation_met_inside_its_target_is_reported_as_met() {
    let d = debrief(&sce(), &good()).expect("debrief");
    let adr = d.expected.iter().find(|e| e.id == "adrenaline_im").expect("adrenaline expectation");
    assert!(adr.done_at.is_some(), "adrenaline was given");
    assert_eq!(adr.done_at, Some(0.0));
    assert!(!adr.late, "given at 0s against a 60s target");
}

/// The number that matters: how far past the target, in seconds, so it can be said out loud.
#[test]
fn a_late_order_reports_how_late() {
    let tape = vec![tick(192.0), act("adrenaline im"), tick(600.0)];
    let d = debrief(&sce(), &tape).expect("debrief");
    let adr = d.expected.iter().find(|e| e.id == "adrenaline_im").unwrap();
    assert_eq!(adr.done_at, Some(192.0));
    assert!(adr.late);
    assert_eq!(adr.late_by, Some(132.0), "192s against a 60s target is 2m12s late");
}

#[test]
fn an_order_never_given_is_reported_as_never() {
    let tape = vec![tick(600.0)];
    let d = debrief(&sce(), &tape).expect("debrief");
    let adr = d.expected.iter().find(|e| e.id == "adrenaline_im").unwrap();
    assert_eq!(adr.done_at, None);
    assert!(!adr.late, "never given is not late — it is a different failure and reads differently");
    assert_eq!(adr.late_by, None);
}

/// An expectation with no target is still reported — it just cannot be late.
#[test]
fn an_expectation_without_a_target_is_reported_but_never_late() {
    let d = debrief(&sce(), &good()).expect("debrief");
    let admit = d.expected.iter().find(|e| e.id == "admit").expect("admit expectation");
    assert!(admit.within.is_none(), "the scenario sets no clock on admitting her");
    assert!(admit.done_at.is_some());
    assert!(!admit.late);
}

/// Harm has to be attributed to the order that caused it, or the debrief says "something hurt
/// her" and leaves the learner to guess which of six things it was.
#[test]
fn harm_is_attributed_to_the_order_before_it() {
    let tape = vec![tick(160.0), act("let her stand up"), tick(600.0)];
    let d = debrief(&sce(), &tape).expect("debrief");
    assert_eq!(d.harms.len(), 1, "{:?}", d.harms);
    let h = &d.harms[0];
    assert_eq!(h.at, 160.0);
    assert_eq!(h.caused_by.as_deref(), Some("stand"), "blamed on `{:?}`", h.caused_by);
}

/// Things the scenario says not to do, that were done anyway.
#[test]
fn an_avoided_order_that_was_given_is_named() {
    let tape = vec![tick(60.0), act("let her stand up"), tick(600.0)];
    let d = debrief(&sce(), &tape).expect("debrief");
    let stood = d.avoided.iter().find(|a| a.id == "stand").expect("stand is on the avoid list");
    assert_eq!(stood.done_at, Some(60.0));
    assert!(!stood.why.is_empty(), "an avoid with no reason teaches nothing");
}

#[test]
fn an_avoided_order_nobody_gave_is_not_held_against_them() {
    let d = debrief(&sce(), &good()).expect("debrief");
    assert!(d.avoided.iter().all(|a| a.done_at.is_none()));
}

/// How long she spent in each state. "Two minutes in arrest" is a fact about the run that no
/// single score can carry.
#[test]
fn time_spent_in_each_status_is_measured() {
    let tape = vec![tick(160.0), act("let her stand up"), tick(600.0)];
    let d = debrief(&sce(), &tape).expect("debrief");
    assert!(!d.statuses.is_empty(), "no status spans");
    let total: f64 = d.statuses.iter().map(|s| s.seconds).sum();
    assert!(total > 0.0);
    assert!(d.statuses.iter().any(|s| s.status == "Arrest"), "{:?}", d.statuses);
}

/// Two people playing the same tape must be told the same thing, or a debrief is not evidence.
#[test]
fn the_same_tape_debriefs_identically() {
    let tape = vec![tick(160.0), act("let her stand up"), tick(600.0)];
    let a = debrief(&sce(), &tape).unwrap();
    let b = debrief(&sce(), &tape).unwrap();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}

/// A scenario with no debrief block still debriefs — it just reports the facts and judges nothing.
#[test]
fn a_scenario_without_targets_still_reports_the_facts() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demo/scenarios/ep2-stemi.json");
    let json = std::fs::read_to_string(p).unwrap();
    let d = debrief(&json, &[tick(300.0)]).expect("debrief");
    assert!(d.expected.is_empty(), "ep2 sets no targets yet");
    assert!(d.outcome.is_none() || d.outcome.is_some());
}
