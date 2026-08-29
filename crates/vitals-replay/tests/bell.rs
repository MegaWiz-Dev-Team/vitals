//! The bell: what happens when a candidate says they are done, and what happens when the clock
//! says it for them.
//!
//! Both are the same act — [`vitals_replay::ring`] — and the property that makes it honest is
//! the one this file exists to hold:
//!
//! > **Ending early and standing at the bedside doing nothing produce the identical tape.**
//!
//! Not a similar tape, not an equivalent score: the same `Vec<Step>`, byte for byte, and
//! therefore the same leaf. That is what makes an early finish carry no penalty (the mark sheet
//! reads exactly as it would have) and no advantage (there is nothing to buy by pressing it a
//! second before the arrest, because the arrest is on the tape either way).
//!
//! A finish that froze the clock could not hold this property, which is the whole argument
//! against building one.

use vitals_replay::{resume, ring, rung, Rang, Step, BELL_CEILING_SEC};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Every case on the shelf, with what its card advertises, in minutes.
///
/// Deliberately a local list rather than a third copy of the server's table: what is under test
/// here is the *mechanism*, and the mechanism has to hold for any duration — which is why every
/// property below is checked at several, including durations no card carries.
fn cases() -> Vec<(&'static str, String)> {
    let mut v: Vec<(&'static str, String)> = ["osce-a", "osce-a2", "osce-b", "osce-b2", "osce-b3",
        "osce-c", "osce-c2", "osce-c3", "osce-d", "osce-d2", "osce-d3", "osce-d4"]
        .iter()
        .map(|id| (*id, format!("{ROOT}/demo/stations/{id}.sce.json")))
        .collect();
    v.push(("ep1", format!("{ROOT}/conformance/sce-anaphylaxis-ep1.json")));
    v.push(("ep2", format!("{ROOT}/demo/scenarios/ep2-stemi.json")));
    v.push(("ep3", format!("{ROOT}/demo/scenarios/ep3-epiglottitis.json")));
    v.push(("ep4", format!("{ROOT}/demo/scenarios/ep4-pulmonary-embolism.json")));
    v.push(("ep5", format!("{ROOT}/demo/scenarios/ep5-the-night-the-stars-fell.json")));
    v
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// What the server's own loop would put on the tape if the candidate stood there until `t`:
/// two-second ticks, and not one step more once the run is over.
fn stood_until(sce: &str, t: f64, limit: f64) -> Vec<Step> {
    let mut st = vitals_sce::SceState::new(vitals_sce::Sce::from_json(sce).expect("sce"));
    let mut tape = Vec::new();
    let mut clock = 0.0;
    while clock < t && st.outcome().is_none() && clock < limit {
        st.tick(2.0);
        tape.push(Step::Tick(2.0));
        clock += 2.0;
    }
    tape
}

/// **The property.** However long the candidate waits before ending it, the run that comes out
/// is the same run — the same ticks, in the same order, to the same conclusion.
///
/// This is what a finish button is allowed to be. It says nothing about the patient; it only
/// stops the candidate contributing, and the case then goes where it was already going.
#[test]
fn ending_early_and_standing_there_produce_the_identical_tape() {
    for (id, path) in cases() {
        let sce = read(&path);
        for limit_min in [8.0, 10.0, 12.0, 14.0, 18.0] {
            let limit = limit_min * 60.0;
            let (whole, why) = rung(&sce, &[], limit).expect("ring");
            for wait_min in [0.5, 1.0, 2.0, 4.0, 7.0, 11.0, 20.0] {
                let prefix = stood_until(&sce, wait_min * 60.0, limit);
                let (also, why2) = rung(&sce, &prefix, limit).expect("ring");
                assert_eq!(
                    (&whole, why), (&also, why2),
                    "{id} @ {limit_min} min: finishing at {wait_min} min is a different run from \
                     finishing at once — an early finish must be worth exactly what standing \
                     there is worth, and this is the only way it can be"
                );
            }
        }
    }
}

/// The bound. Every case in this repository reaches an ending of its own or has time called on
/// it; none of them runs into the backstop, which is what the backstop being a backstop means.
#[test]
fn every_case_reaches_an_ending_without_touching_the_ceiling() {
    for (id, path) in cases() {
        let sce = read(&path);
        for limit_min in [8.0, 10.0, 14.0, 18.0] {
            let (whole, why) = rung(&sce, &[], limit_min * 60.0).expect("ring");
            assert_ne!(why, Rang::Ceiling, "{id}: the bell ran to the backstop");
            let (_, r) = resume(&sce, &whole).expect("replay");
            assert!(
                r.sim_seconds < BELL_CEILING_SEC,
                "{id}: resolving took {} simulated seconds",
                r.sim_seconds
            );
        }
    }
}

/// The two stations the audit ran to sixty simulated minutes with no outcome and the mark sheet
/// still sealed. Neither declares an ending edge a candidate can reach by standing still, and
/// no scenario file is touched to give them one: the clock ends them, at the mark their card
/// advertises and not a tick later, because there is nothing left about either patient to
/// resolve.
#[test]
fn the_two_stations_that_never_ended_now_end_on_their_own_clock() {
    for id in ["osce-b2", "osce-c"] {
        let sce = read(&format!("{ROOT}/demo/stations/{id}.sce.json"));
        let (whole, why) = rung(&sce, &[], 600.0).expect("ring");
        let (st, r) = resume(&sce, &whole).expect("replay");
        assert_eq!(why, Rang::TimeCalled, "{id} found an ending it does not have");
        assert!(st.outcome().is_none(), "{id} invented a terminal outcome");
        assert_eq!(r.sim_seconds, 600.0, "{id}: time was called somewhere other than the bell");
    }
}

/// The other half of that: a case still *doing* something at the advertised mark is not cut off
/// there. `osce-c3`'s failing narrative arrests at fourteen simulated minutes against a
/// ten-minute card, and it still arrests — the candidate's ten minutes are up, the patient's
/// are not, and a station that stopped her mid-slide to satisfy a label would be handing back
/// a survivor the run did not earn.
#[test]
fn a_case_still_moving_at_the_bell_is_not_cut_off_at_it() {
    for (id, want) in [("osce-b", 696.0), ("osce-b3", 662.0), ("osce-c2", 722.0), ("osce-c3", 842.0)] {
        let sce = read(&format!("{ROOT}/demo/stations/{id}.sce.json"));
        let (whole, why) = rung(&sce, &[], 600.0).expect("ring");
        let (st, r) = resume(&sce, &whole).expect("replay");
        assert_eq!(why, Rang::Outcome, "{id} had time called on a patient who was still moving");
        assert_eq!(st.outcome_id(), Some("death_arrest"), "{id} reached the wrong ending");
        assert_eq!(r.sim_seconds, want, "{id} arrested somewhere new");
    }
}

/// The horizon is read out of the case, not guessed. `ep1` is the only scenario in the season
/// that sets a self-clearing flag, and the quiet window has to outlast it — a flag that expires
/// after we stopped watching would switch a dynamic back on over a patient already called.
#[test]
fn the_quiet_window_outlasts_the_longest_flag_a_case_sets() {
    let sce = read(&format!("{ROOT}/conformance/sce-anaphylaxis-ep1.json"));
    let h = vitals_replay::horizon(&vitals_sce::Sce::from_json(&sce).unwrap());
    assert!(h.quiet > 300.0, "ep1 sets a 300 s flag and the window is only {}", h.quiet);
    // And a case with none keeps the floor.
    let plain = read(&format!("{ROOT}/demo/stations/osce-b2.sce.json"));
    let h2 = vitals_replay::horizon(&vitals_sce::Sce::from_json(&plain).unwrap());
    assert_eq!(h2.quiet, vitals_replay::bell::QUIET_MIN_SEC);
}

/// Ringing a bell that has already rung adds nothing. The server only ever calls it on the
/// crossing, but "cannot happen" is a claim and this is the property.
#[test]
fn a_bell_already_rung_adds_no_more_tape() {
    let sce = read(&format!("{ROOT}/demo/stations/osce-b2.sce.json"));
    let (whole, _) = rung(&sce, &[], 600.0).expect("ring");
    let (mut st, _) = resume(&sce, &whole).expect("replay");
    let (added, beats, why) = ring(&sce, &mut st, 600.0).expect("ring");
    assert!(added.is_empty() && beats.is_empty(), "the bell rang twice: {added:?}");
    assert_eq!(why, Rang::TimeCalled);
}
