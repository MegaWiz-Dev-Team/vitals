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
type Chart = (Option<String>, String, usize, Vec<(String, Option<f64>)>);

fn seen(st: &vitals_sce::runtime::SceState) -> Chart {
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
    shift(&mut handed_over, &second(), 0.0);

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

    let r2 = shift(&mut st, &second(), 0.0);

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
    shift(&mut chained, &b, 0.0);
    shift(&mut chained, &c, 0.0);

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

use vitals_replay::{idle_sim_seconds, idle_sim_seconds_on_arrival, pass_idle,
                    ARRIVAL_IDLE_CAP_SIM_SECONDS, IDLE_SIM_PER_REAL};

/// A patient nobody visits is not the patient you left.
///
/// Between shifts her body does not stop; it advances slowly, deterministically, from the gap the
/// chain itself records — so the ward is a ward, and every browser still re-derives the same
/// patient because the gap is on chain and the ratio is a constant.
#[test]
fn a_gap_between_shifts_changes_her_and_a_short_one_does_not() {
    let sce = ep1();

    let (mut back_to_back, _) = resume(&sce, &first()).expect("shift one");
    shift(&mut back_to_back, &second(), 0.0);

    let (mut after_a_night, _) = resume(&sce, &first()).expect("shift one");
    shift(&mut after_a_night, &second(), 8.0 * 3600.0);

    assert_ne!(seen(&back_to_back), seen(&after_a_night),
               "eight hours alone must leave a different patient than a straight handover");
    assert!(after_a_night.t_sec() > back_to_back.t_sec(),
            "and the difference is time she spent untreated");
}

/// **The cap is gone** (founder, 16 ก.ย. 11:30). A gap is worth exactly what it lasted.
///
/// It used to stop at two simulated minutes, set below the fastest untreated arrest so that time
/// alone could deteriorate a patient and never kill one. The founder removed it: at 1:60 a patient
/// nobody visits deteriorates as the engine says, and can arrest and die with nobody in the room.
///
/// **The gap is real seconds, and the chain is what says how many.** It was a slot count times
/// 0.4 s, and devnet spent 17 ก.ย. producing slots at 0.166 s — so a patient left alone for ten
/// real minutes was being advanced as though twenty-four had passed, and every death the ticker
/// wrote down came 2.4× too early. The two block times are chain facts, cached and never
/// recomputed, so a stranger who reads the two slots off the chain and asks the same RPC for their
/// times re-derives the same patient.
#[test]
fn the_idle_clock_is_slow_linear_and_derivable() {
    assert_eq!(idle_sim_seconds(0.0), 0.0, "a handover with no gap adds nothing");

    // sixty real minutes → one simulated minute
    assert!((idle_sim_seconds(3600.0) - 60.0).abs() < 1e-9,
            "the stated ratio is one simulated minute per sixty real ones");
    assert!((IDLE_SIM_PER_REAL - 1.0 / 60.0).abs() < 1e-9);

    // No ceiling: a weekend alone is a weekend alone, and how long we were away is exactly what
    // it costs her. Three days at 1:60 is seventy-two simulated minutes.
    assert!((idle_sim_seconds(3.0 * 24.0 * 3600.0) - 72.0 * 60.0).abs() < 1e-6,
            "three days away is seventy-two simulated minutes, not a ceiling: {}",
            idle_sim_seconds(3.0 * 24.0 * 3600.0));
    assert!((idle_sim_seconds(7200.0) - 120.0).abs() < 1e-9,
            "and two real hours is two simulated minutes — the number the old cap froze at, which \
             is now a point on the line rather than the end of it: {}",
            idle_sim_seconds(7200.0));
    assert!(idle_sim_seconds(3.0 * 24.0 * 3600.0) > idle_sim_seconds(7200.0) * 30.0,
            "strictly longer gaps must cost strictly more, or 'she was alone all weekend' means \
             nothing the record can show");

    // A negative span is a clock going backwards, never a patient getting younger.
    assert_eq!(idle_sim_seconds(-90.0), 0.0);
}

/// **A gap costs her everything it lasted, and costs whoever finally comes at most five minutes.**
///
/// Two rules about the same gap, and the ward needs both to be true at once. The first stranger
/// ever to take a shift here, on 22 Sep 2026, opened Nadege Toussaint after she had been alone for
/// twelve real hours: at 1:60 that is twelve simulated minutes into a PSVT case, so she was already
/// past the point the case can be treated from. He asked her nine questions over forty seconds,
/// she arrested, and he left without recording anything. A ward where the first shift is always a
/// death teaches one thing, which is not to come back.
///
/// The founder's ruling, 23 Sep 2026: "รักษาหลักการ 'ไม่มีใครมาก็ตาย' ไว้ แต่ให้คนที่มาถึงได้รักษาจริง
/// ไม่ใช่มาดูตาย" — keep the principle that nobody coming means she dies, but whoever does come gets
/// to treat, not to watch a death.
///
/// So: `idle_sim_seconds` stays uncapped and keeps its own test above, because that is the rule the
/// ticker applies when it decides she has died alone. `idle_sim_seconds_on_arrival` is the rule for
/// a chart being brought up to the moment somebody arrived, and it stops at five simulated minutes.
/// The two functions exist separately so neither can be quietly used for the other's job.
#[test]
fn a_stranger_who_arrives_finds_her_five_minutes_in_at_the_most() {
    // Below the cap the two rules agree exactly — the cap is a ceiling, never a rescaling, so a
    // patient alone for one hour is one simulated minute in on both clocks.
    for real in [0.0, 60.0, 3600.0, 4.0 * 3600.0, 5.0 * 3600.0] {
        assert!((idle_sim_seconds_on_arrival(real) - idle_sim_seconds(real)).abs() < 1e-9,
                "under the cap an arrival sees exactly what the gap cost: {real} real seconds");
    }

    // Five real hours is the hinge: 300 simulated seconds, which is the cap itself.
    assert!((idle_sim_seconds_on_arrival(5.0 * 3600.0) - 300.0).abs() < 1e-9);
    assert!((ARRIVAL_IDLE_CAP_SIM_SECONDS - 300.0).abs() < 1e-9,
            "five simulated minutes, the founder's number on 23 Sep");

    // Past it, every gap is worth the same to the person who walks in — which is the whole point.
    // Nadege's twelve real hours and a patient abandoned over a long weekend hand the same state to
    // whoever opens the bed, and it is a state with a shift's worth of time left in it.
    for real in [5.0 * 3600.0 + 1.0, 12.0 * 3600.0, 3.0 * 24.0 * 3600.0] {
        assert!((idle_sim_seconds_on_arrival(real) - 300.0).abs() < 1e-9,
                "a gap past the cap hands over five simulated minutes, not {}",
                idle_sim_seconds_on_arrival(real));
    }

    // And the uncapped rule is still uncapped, because the ticker still has to be able to finish
    // her. If this ever stops holding, being abandoned on this ward became survivable by accident.
    assert!(idle_sim_seconds(12.0 * 3600.0) > ARRIVAL_IDLE_CAP_SIM_SECONDS,
            "twelve hours alone is still twelve simulated minutes to the engine that closes beds");

    // A negative span is a clock disagreeing with itself here too, never five free minutes.
    assert_eq!(idle_sim_seconds_on_arrival(-90.0), 0.0);
}

#[test]
fn the_same_gap_always_gives_the_same_patient() {
    let sce = ep1();
    let gap = 3_600.0;   // one real hour between the two shifts, as the chain dates them
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

    // Tested on the mechanism rather than through `shift`, deliberately. This is the one rule on
    // the ward that must hold whatever the ratio and whatever the gap — there is one physiology,
    // and time nobody watched runs the same engine as time somebody did. Pinned at `pass_idle` so
    // it cannot come to depend on a constant somebody is entitled to move, and the founder moved
    // two of them in one day.
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
/// ward turns its beds over slowly enough that the queue is not eaten by time passing. There is no
/// longer a ceiling over it: the founder removed the cap the same day, so the line runs on.
///
/// Pinned as two numbers a stranger can check with a calculator, because this is the constant most
/// likely to be moved again and the plan quotes these exact figures.
#[test]
fn an_hour_away_costs_her_a_minute() {
    assert!((idle_sim_seconds(3600.0) - 60.0).abs() < 1e-6,
            "sixty real minutes must advance her exactly sixty simulated seconds, and this is \
             1:60 written where a reader can divide it themselves");

    assert!((idle_sim_seconds(10.0 * 3600.0) - 600.0).abs() < 1e-3,
            "ten hours away is ten simulated minutes — and ten simulated minutes is past the \
             arrest of most of the catalogue, which is the founder's ruling of 16 ก.ย. and not a \
             side effect: a patient nobody visits can die of being nobody's patient");

    assert!((idle_sim_seconds(2.0 * 3600.0) - 120.0).abs() < 1e-3,
            "two real hours is two simulated minutes: 1:60, written where a reader can divide it");
}
