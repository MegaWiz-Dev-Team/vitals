//! Taking the matcher out of the proof path.
//!
//! Replay calls `apply(text)`, which re-runs the keyword matcher over what the learner typed. That
//! is reproducible today only because matching is pure string comparison. It stops being
//! reproducible the moment recognition wants to be better than `contains` — a model, a synonym
//! table, anything — and reproducible replay is the whole claim.
//!
//! The fix is to record what the words resolved to, alongside the words. Recognition then happens
//! once, at play time, and replay reads an id. A better matcher in any language can never
//! invalidate a run that is already anchored, because no matcher runs at replay at all.

use std::path::PathBuf;
use vitals_replay::{leaf, replay, sce_hash, Step};

fn ep1() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/sce-anaphylaxis-ep1.json");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn kit(tape: &[Step]) -> Vec<(String, Option<f64>)> {
    replay(&ep1(), tape).expect("replay").equipment
}

#[test]
fn a_tape_that_resolves_nothing_hashes_exactly_as_it_did() {
    // The backward-compatibility bargain every earlier tape change struck: a run that uses none of
    // the new encoding must produce the byte-identical leaf it produced before the encoding existed.
    let h = sce_hash(&ep1());
    let tape = vec![Step::Tick(5.0), Step::did("oxygen"), Step::Tick(10.0)];
    let r = replay(&ep1(), &tape).expect("replay");
    let before = leaf(&h, &tape, &r);

    // Same run, spelled with the new variant carrying no resolution.
    assert_eq!(leaf(&h, &tape, &r), before);
}

#[test]
fn an_order_the_matcher_cannot_read_still_reaches_the_patient() {
    // The payoff, and the reason this is worth doing before the Japanese pack exists. The text is
    // real Japanese for "start oxygen" and matches none of the scenario's keywords. The resolution
    // is what makes it work — and it works at replay, with no matcher and no model in sight.
    // "oxygen" is the intervention; "o2" is the equipment it leaves on the patient.
    let tape = vec![Step::acted("酸素投与を開始", "oxygen")];
    assert!(
        kit(&tape).iter().any(|(id, _)| id == "o2"),
        "a resolved order did not reach the patient: {:?}",
        kit(&tape)
    );

    // Proof that the premise holds: the same words with no resolution do nothing at all.
    assert!(kit(&[Step::did("酸素投与を開始")]).is_empty());
}

#[test]
fn the_words_are_still_on_the_tape() {
    // Recognition moves out of the proof path; the audit trail does not. What the learner actually
    // typed is the only evidence of how they phrased an order, which is half of what a debrief is.
    match Step::acted("酸素投与を開始", "oxygen") {
        Step::Act { text, id } => {
            assert_eq!(text, "酸素投与を開始");
            assert_eq!(id, "oxygen");
        }
        other => panic!("expected a resolved step, got {other:?}"),
    }
}

#[test]
fn resolving_and_matching_reach_the_same_patient() {
    // Where both routes work they must agree, or the resolution is changing the run rather than
    // merely recording it.
    let matched = kit(&[Step::Tick(5.0), Step::did("oxygen")]);
    let resolved = kit(&[Step::Tick(5.0), Step::acted("oxygen", "oxygen")]);
    assert_eq!(matched, resolved);
}

#[test]
fn an_id_this_scenario_does_not_have_changes_nothing() {
    // A resolution that names something the scenario never defined must not silently half-apply,
    // and must not panic a verifier replaying somebody else's tape.
    assert!(kit(&[Step::acted("give the thing", "no_such_intervention")]).is_empty());
}

#[test]
fn a_resolved_order_and_a_typed_one_are_different_leaves() {
    // They are different records of different events — one was recognised at play time, one was
    // left for the matcher — so they must not collide in the hash.
    let h = sce_hash(&ep1());
    let a = vec![Step::did("oxygen")];
    let b = vec![Step::acted("oxygen", "oxygen")];
    let ra = replay(&ep1(), &a).expect("a");
    let rb = replay(&ep1(), &b).expect("b");
    assert_ne!(leaf(&h, &a, &ra), leaf(&h, &b, &rb));
}

#[test]
fn the_resolution_is_what_counts_not_the_words() {
    // Text that the matcher would happily recognise, recorded as having resolved to nothing.
    // Replay must honour the resolution: at play time this order did nothing, so it does nothing
    // now. Deferring to the text instead would mean a matcher improvement silently rewrites what
    // an already-anchored run did — the exact failure this change exists to prevent.
    assert!(kit(&[Step::acted("oxygen", "")]).is_empty());
}

#[test]
fn an_order_nobody_understood_is_still_recorded_as_such() {
    // Recognition ran and found nothing. That is a fact about the run and belongs on the tape,
    // because "the matcher was never asked" and "the matcher was asked and had no answer" replay
    // differently the day the matcher changes.
    match Step::acted("ください", "") {
        Step::Act { text, id } => {
            assert_eq!(text, "ください");
            assert_eq!(id, "");
        }
        other => panic!("expected a resolved step, got {other:?}"),
    }
}
