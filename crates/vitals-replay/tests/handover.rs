//! A stay is a chain of shifts, and the chain must equal the whole.
//!
//! The ward hands a patient from one stranger to the next (CWF_PLAN.md). Shift N+1 begins on the
//! machine shift N left behind, and the guarantee that makes the chart trustworthy is this: two
//! tapes played one after the other land the patient exactly where one tape of both would have.
//! If that ever stops being true, the chart a stranger rebuilds is not the patient they are
//! treating, and every claim on the page is void.

use std::path::PathBuf;
use vitals_replay::{replay, resume, shift, Step};

fn ep1() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/sce-anaphylaxis-ep1.json");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// What a stranger opening her would see: where she is, what is on her, and what she has been
/// through. Compared rather than the whole state, because these are the things the next shift acts
/// on and the things the leaf commits to.
fn seen(st: &vitals_sce::runtime::SceState) -> (Option<String>, String, usize, Vec<(String, Option<f64>)>) {
    (
        st.outcome().map(|o| format!("{o:?}")),
        format!("{:.3}", st.t_sec()),
        st.harm_events.len(),
        st.equipment().iter().map(|e| (e.id.clone(), e.setting)).collect(),
    )
}

fn first() -> Vec<Step> {
    vec![
        Step::Tick(30.0),
        Step::Do("oxygen".into()),
        Step::Tick(45.0),
        Step::Do("iv access".into()),
        Step::Tick(30.0),
    ]
}

fn second() -> Vec<Step> {
    vec![
        Step::Tick(20.0),
        Step::Do("adrenaline im".into()),
        Step::Tick(60.0),
        Step::Do("fluids".into()),
        Step::Tick(40.0),
    ]
}

#[test]
fn two_shifts_leave_her_exactly_where_one_tape_of_both_would() {
    let sce = ep1();
    let whole: Vec<Step> = first().into_iter().chain(second()).collect();

    let (one_go, _) = resume(&sce, &whole).expect("one tape");
    let (mut handed_over, _) = resume(&sce, &first()).expect("shift one");
    shift(&mut handed_over, &second());

    assert_eq!(seen(&handed_over), seen(&one_go),
               "the patient the second stranger left is not the patient one tape would have left");
}

#[test]
fn a_shift_is_scored_on_what_it_did_not_on_what_it_walked_into() {
    let sce = ep1();
    // Shift one stands her up — the scenario calls that harm ("stand/walk collapse"), and it is
    // exactly the kind of thing the next stranger inherits and must not be blamed for.
    let harmful: Vec<Step> = vec![Step::Tick(30.0), Step::Do("stand her up".into()), Step::Tick(30.0)];
    let (mut st, r1) = resume(&sce, &harmful).expect("shift one");
    let inherited = st.harm_events.len();

    let r2 = shift(&mut st, &second());

    assert!(inherited > 0,
            "this test is worthless unless shift one actually harmed her — it did not, so the \
             scenario or the tape changed and the test must be rewritten, not deleted");
    assert_eq!(r1.harm_events.len(), inherited, "shift one reports the harm shift one did");
    assert_eq!(r2.harm_events.len(), st.harm_events.len() - inherited,
               "shift two reports only the harm it added, never the harm it inherited");
    assert_eq!(r2.steps, second().len(), "and only its own steps");
    assert_eq!(r2.sim_seconds, 120.0, "and only its own seconds");
}

#[test]
fn a_chain_of_three_still_equals_the_whole() {
    let sce = ep1();
    let a = vec![Step::Tick(25.0), Step::Do("oxygen".into())];
    let b = vec![Step::Tick(35.0), Step::Do("iv access".into())];
    let c = vec![Step::Tick(40.0), Step::Do("adrenaline im".into()), Step::Tick(50.0)];
    let whole: Vec<Step> = a.iter().chain(&b).chain(&c).cloned().collect();

    let (one_go, _) = resume(&sce, &whole).expect("one tape");
    let (mut chained, _) = resume(&sce, &a).expect("shift a");
    shift(&mut chained, &b);
    shift(&mut chained, &c);

    assert_eq!(seen(&chained), seen(&one_go), "three shifts must equal one tape of all three");
}

/// The verifier's view is unchanged by any of this: `replay` still answers for a whole tape, and a
/// handed-over stay is a whole tape when you have all of it.
#[test]
fn the_verifier_still_sees_one_tape() {
    let sce = ep1();
    let whole: Vec<Step> = first().into_iter().chain(second()).collect();
    let r = replay(&sce, &whole).expect("replay");
    assert_eq!(r.steps, first().len() + second().len());
    let (st, _) = resume(&sce, &whole).expect("resume");
    assert_eq!(r.outcome, st.outcome().map(|o| format!("{o:?}")));
}
