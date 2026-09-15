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
    shift(&mut handed_over, &second(), 0);

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

    let r2 = shift(&mut st, &second(), 0);

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
    shift(&mut chained, &b, 0);
    shift(&mut chained, &c, 0);

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

// ── the idle clock ──────────────────────────────────────────────────────────

use vitals_replay::{idle_seconds, pass_idle, IDLE_CAP_SIM_SECONDS, IDLE_SIM_PER_REAL, SLOT_SECONDS};

/// A patient nobody visits is not the patient you left.
///
/// Between shifts her body does not stop; it advances slowly, deterministically, from the gap the
/// chain itself records — so the ward is a ward, and every browser still re-derives the same
/// patient because the gap is on chain and the ratio is a constant.
#[test]
fn a_gap_between_shifts_changes_her_and_a_short_one_does_not() {
    let sce = ep1();

    let (mut back_to_back, _) = resume(&sce, &first()).expect("shift one");
    shift(&mut back_to_back, &second(), 0);

    let (mut after_a_night, _) = resume(&sce, &first()).expect("shift one");
    let eight_hours_of_slots = (8.0 * 3600.0 / SLOT_SECONDS) as u64;
    shift(&mut after_a_night, &second(), eight_hours_of_slots);

    assert_ne!(seen(&back_to_back), seen(&after_a_night),
               "eight hours alone must leave a different patient than a straight handover");
    assert!(after_a_night.t_sec() > back_to_back.t_sec(),
            "and the difference is time she spent untreated");
}

#[test]
fn the_idle_clock_is_slow_bounded_and_derivable() {
    assert_eq!(idle_seconds(0), 0.0, "a handover with no gap adds nothing");

    // ten real minutes of slots → one simulated minute
    let ten_minutes = (600.0 / SLOT_SECONDS) as u64;
    assert!((idle_seconds(ten_minutes) - 60.0).abs() < 1.0,
            "the stated ratio is one simulated minute per ten real ones");
    assert!((IDLE_SIM_PER_REAL - 0.1).abs() < 1e-9);

    // a weekend alone is the same two minutes twenty real minutes buys
    let three_days = (3.0 * 24.0 * 3600.0 / SLOT_SECONDS) as u64;
    let twenty_minutes = (1200.0 / SLOT_SECONDS) as u64;
    assert_eq!(idle_seconds(three_days), IDLE_CAP_SIM_SECONDS,
               "a long weekend alone must leave her where twenty real minutes leaves her — nothing \
                about an abandoned patient may depend on how long we were away");
    assert_eq!(idle_seconds(twenty_minutes), IDLE_CAP_SIM_SECONDS,
               "at this ratio the cap is reached after twenty real minutes, and every longer gap \
                is that same gap");
    assert_eq!(IDLE_CAP_SIM_SECONDS, 120.0,
               "two simulated minutes, set below the fastest untreated arrest in the catalogue \
                (ep5 at 186 s) so a gap can only ever deteriorate her — vitals-web's \
                no_case_in_the_catalogue_dies_of_the_idle_clock_alone is what holds that against \
                all sixteen cases");
}

#[test]
fn the_same_gap_always_gives_the_same_patient() {
    let sce = ep1();
    let gap = 9_000u64;
    let run = || {
        let (mut st, _) = resume(&sce, &first()).expect("shift one");
        shift(&mut st, &second(), gap);
        seen(&st)
    };
    assert_eq!(run(), run(), "two browsers, one gap, one patient — or none of this is verifiable");
}

/// Idle time must pass the way time on shift passes.
///
/// The engine is a tick machine: one state edge per tick, triggers evaluated once per tick. Hand
/// it a whole hour as a single tick and it sails straight past the arrest it should have run into
/// — ep1 at a one-hour gap comes back with a systolic of 0, a saturation of 0 and **no outcome at
/// all**, a corpse the chart still calls alive, while the same hour ticked at the scenario's own
/// grain arrests her at 518 seconds. Five of the sixteen catalogue cases behave the same way.
///
/// That would make the ward's clock a different machine from the player's, and the chain rests on
/// there being only one machine. So the gap is ticked in the scenario's grain, and this is the
/// test that says so.
#[test]
fn idle_time_passes_the_way_time_on_shift_passes() {
    let sce = ep1();
    let an_hour_on_shift: Vec<Step> = (0..3600).map(|_| Step::Tick(1.0)).collect();
    let (played, _) = resume(&sce, &an_hour_on_shift).expect("an hour, played");

    let (mut idled, _) = resume(&sce, &[]).expect("fresh");
    pass_idle(&mut idled, 3600.0);

    assert_eq!(seen(&idled), seen(&played),
               "an hour alone must leave exactly the patient an hour on shift leaves — if idle time \
                runs the engine coarsely it is a second physiology, and the chart a stranger \
                re-derives is not the patient in the bed");

    // Tested on the mechanism rather than through `shift`, deliberately. The cap is two simulated
    // minutes and no catalogue case tells a single tick of that from two minutes of ticks, so the
    // same assertion written through `shift` would pass with the bug back in and guard nothing.
    // Raise the cap past ep5's 186 s and it would start guarding again; the grain rule holds at
    // every cap, so it is pinned where it does not depend on one.
    let (coarse, _) = resume(&sce, &[Step::Tick(3600.0)]).expect("one jump");
    assert_ne!(seen(&coarse), seen(&played),
               "this test is worthless unless one big tick actually differs from an hour of ticks \
                — it no longer does, so the engine changed and the test must be rewritten, not \
                deleted");
}

/// The ratio, as the founder set it: **one simulated minute per sixty real ones** (16 ก.ย.).
///
/// Slower than the first setting by six times, and the reason is the ward rather than the
/// physiology: a patient who drifts slowly is a patient several strangers can still meet, and the
/// ward turns its beds over slowly enough that the queue is not eaten by time passing. The cap is
/// unchanged, so it is now reached after **two real hours** rather than twenty real minutes.
///
/// Pinned as two numbers a stranger can check with a calculator, because this is the constant most
/// likely to be moved again and the plan quotes these exact figures.
#[test]
fn an_hour_away_costs_her_a_minute() {
    let one_real_hour = (3600.0 / SLOT_SECONDS) as u64;
    assert!((idle_seconds(one_real_hour) - 60.0).abs() < 1e-6,
            "sixty real minutes must advance her exactly sixty simulated seconds, and this is \
             1:60 written where a reader can divide it themselves");

    let ten_real_hours = (10.0 * 3600.0 / SLOT_SECONDS) as u64;
    assert_eq!(idle_seconds(ten_real_hours), IDLE_CAP_SIM_SECONDS,
               "and ten hours away is the cap, which is two simulated minutes");

    let two_real_hours = (2.0 * 3600.0 / SLOT_SECONDS) as u64;
    assert_eq!(idle_seconds(two_real_hours), IDLE_CAP_SIM_SECONDS,
               "the cap is reached at two real hours: 120 simulated seconds at one per sixty");
}
