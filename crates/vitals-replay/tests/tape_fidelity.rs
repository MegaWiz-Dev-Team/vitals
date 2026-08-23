//! The tape has to replay to the machine the player was looking at.
//!
//! Everything Vitals claims rests on one sentence: the run is deterministic, so a verifier can
//! replay the tape and re-derive the outcome. That sentence is false the moment the tape stores
//! something the replay reads differently — and free text stored for a device does exactly that,
//! because free text goes back through the intervention matcher and the matcher keys on the
//! device's own name.
//!
//! These are the two ways it broke, kept as tests so they cannot come back.

use std::path::PathBuf;
use vitals_replay::{leaf, replay, sce_hash, Step};

fn ep1() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/sce-anaphylaxis-ep1.json");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn kit(tape: &[Step]) -> Vec<(String, Option<f64>)> {
    replay(&ep1(), tape).expect("replay").equipment
}

/// The scenario's canonical flow is 10 L/min. A player who dials 6 must replay to 6.
#[test]
fn a_dialled_setting_survives_the_tape() {
    let tape = vec![
        Step::Do("oxygen face mask 6 lpm".into()),
        Step::Set("o2".into(), 6.0),
    ];
    assert_eq!(kit(&tape), vec![("o2".to_string(), Some(6.0))]);
}

/// Without the `Set` step the number lived only in the web process: the player saw 6, the leaf
/// certified 10. This is that bug, pinned.
#[test]
fn the_order_text_alone_replays_to_the_scenario_default() {
    let tape = vec![Step::Do("oxygen face mask 6 lpm".into())];
    assert_eq!(kit(&tape), vec![("o2".to_string(), Some(10.0))],
               "the matcher takes its number from the scenario, never from the order text");
}

/// Taking the mask off must take the mask off.
#[test]
fn off_removes_the_device() {
    let tape = vec![
        Step::Do("oxygen face mask 10 lpm".into()),
        Step::Off("o2".into()),
    ];
    assert!(kit(&tape).is_empty(), "the player took it off");
}

/// The same instruction as free text does the *opposite*: "remove o2" hits the oxygen
/// intervention's own `o2` keyword and puts the mask back on. A tape that inverts an action is
/// worse than one that drops it, so this stays pinned too.
#[test]
fn remove_as_free_text_puts_it_back_on() {
    let tape = vec![
        Step::Do("oxygen face mask 10 lpm".into()),
        Step::Do("remove o2".into()),
    ];
    assert_eq!(kit(&tape), vec![("o2".to_string(), Some(10.0))],
               "this is why Step::Off exists and free text cannot be trusted for devices");
}

/// A tape that never touches a dial must encode exactly as it did before dials were on the tape,
/// or every leaf anchored under the older encoding stops verifying.
///
/// The scenario hash is held at zero on purpose. This test is about the *tape* encoding, and an
/// earlier version fed it the real scenario — so editing the scenario broke a test that claims to
/// be about something else, and the failure looked like an encoding regression when it was a
/// scenario edit doing exactly what the scenario hash exists to do.
#[test]
fn tapes_without_device_steps_encode_unchanged() {
    let sce = ep1();
    let tape = vec![
        Step::Tick(30.0),
        Step::Do("adrenaline im".into()),
        Step::Tick(300.0),
    ];
    let r = replay(&sce, &tape).unwrap();
    assert_eq!(
        vitals_replay::hex(&leaf(&[0u8; 32], &tape, &r)),
        "e759e9c138a97e014a413def8b49e9826cbcec29957dfe740b1eb8d8fdcb234e",
        "computed by the pre-Set/Off code over this same tape"
    );
}

/// And the scenario hash still does its job: rewriting the scenario must change the leaf, or a
/// rewritten case could silently revalue every credential ever issued against it.
#[test]
fn a_different_scenario_is_a_different_leaf() {
    let sce = ep1();
    let tape = vec![Step::Tick(30.0), Step::Do("adrenaline im".into())];
    let r = replay(&sce, &tape).unwrap();
    let a = leaf(&sce_hash(&sce), &tape, &r);
    let b = leaf(&sce_hash(&format!("{sce} ")), &tape, &r);
    assert_ne!(a, b, "a changed scenario produced the same leaf");
}

/// The exact tape the web server now writes when a player dials O2 to 6, starts an IV at 250,
/// then takes the mask off. Replaying it has to land on the same kit the player was looking at.
#[test]
fn the_servers_own_kit_tape_round_trips() {
    let tape = vec![
        Step::Do("oxygen face mask 6 lpm".into()),
        Step::Set("o2".into(), 6.0),
        Step::Do("iv access normal saline 250 ml/hr".into()),
        Step::Set("iv".into(), 250.0),
        Step::Off("o2".into()),
    ];
    assert_eq!(kit(&tape), vec![("iv".to_string(), Some(250.0))]);
}
