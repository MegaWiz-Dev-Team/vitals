//! What a body does when it stops.
//!
//! The automaton reached a death outcome and then simply stopped ticking, which left every vital
//! frozen at the value it held the instant before. The screen showed a patient marked Dead with a
//! heart rate of 128, a respiratory rate of 28, and a saturation of 88% — the monitor still
//! sweeping, the chest still rising.
//!
//! It also let diastolic pressure exceed systolic. `50/54` is not a low blood pressure, it is an
//! impossible one, and it was on screen.

use std::path::PathBuf;
use vitals_sce::{Sce, SceState};

fn ep1() -> Sce {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/sce-anaphylaxis-ep1.json");
    Sce::from_json(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
        .expect("parse")
}

/// Play EP1 badly enough to lose her.
fn kill() -> SceState {
    let mut st = SceState::new(ep1());
    st.apply("let her stand up");
    for _ in 0..40 {
        st.tick(30.0);
        if st.outcome().is_some() {
            break;
        }
    }
    assert!(st.outcome().is_some(), "the tape did not reach a terminal state");
    st
}

#[test]
fn a_dead_patient_has_no_vital_signs() {
    let st = kill();
    let v = st.vitals;
    assert_eq!(v.hr, 0.0, "a dead patient has no heart rate");
    assert_eq!(v.rr, 0.0, "a dead patient is not breathing");
    assert_eq!(v.sbp, 0.0, "a dead patient has no blood pressure");
    assert_eq!(v.dbp, 0.0);
    assert_eq!(v.spo2, 0.0, "there is no pulse for an oximeter to read");
}

#[test]
fn a_dead_patient_is_in_asystole() {
    let st = kill();
    assert!(!st.vitals.rhythm.perfusing(), "a perfusing rhythm on a dead patient");
    assert!(!st.vitals.rhythm.shockable(), "offering a shock to a dead patient");
    assert_eq!(st.vitals.rhythm.as_str(), "asystole");
}

/// Surviving must not zero anybody out — the rule is about death, not about the run ending.
#[test]
fn a_patient_who_lives_keeps_her_vitals() {
    let mut st = SceState::new(ep1());
    for a in ["adrenaline im", "oxygen", "supine"] {
        st.apply(a);
    }
    st.tick(60.0);
    st.apply("normal saline bolus");
    st.tick(300.0);
    st.apply("admit for observation");
    for _ in 0..8 {
        st.tick(300.0);
        if st.outcome().is_some() {
            break;
        }
    }
    assert!(st.outcome().is_some(), "the run never finished");
    assert!(st.vitals.hr > 0.0, "a discharged patient with no pulse");
    assert!(st.vitals.spo2 > 0.0);
}

/// Diastolic above systolic is arithmetic nonsense, not a severe reading, and it reached the
/// screen as `50/54`.
#[test]
fn diastolic_never_exceeds_systolic() {
    let mut st = SceState::new(ep1());
    st.apply("let her stand up");
    for _ in 0..60 {
        st.tick(10.0);
        assert!(
            st.vitals.dbp <= st.vitals.sbp,
            "{}/{} — diastolic above systolic",
            st.vitals.sbp.round(),
            st.vitals.dbp.round()
        );
        if st.outcome().is_some() {
            break;
        }
    }
}
