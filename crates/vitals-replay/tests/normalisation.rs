//! A run must hash to the same leaf whatever keyboard recorded it.
//!
//! The leaf is built from the tape, and the tape holds the text the learner typed. Two devices
//! can send the same Korean word as different bytes — precomposed on most keyboards, decomposed
//! on macOS and some Android IMEs — and a Japanese IME sends full-width digits where a US
//! keyboard sends ASCII. Left alone, that means two identical runs anchor two different leaves,
//! and a learner who switches phones can no longer prove what they did on the old one.

use vitals_replay::Step;

const PRECOMPOSED: &str = "\u{C544}\u{C2A4}\u{D53C}\u{B9B0}";
const DECOMPOSED: &str = "\u{110B}\u{1161}\u{1109}\u{1173}\u{1111}\u{1175}\u{1105}\u{1175}\u{11AB}";

#[test]
fn the_same_order_from_two_keyboards_is_one_step() {
    assert_ne!(PRECOMPOSED, DECOMPOSED, "the test premise: these differ as bytes");
    assert_eq!(Step::did(PRECOMPOSED), Step::did(DECOMPOSED));
}

#[test]
fn a_full_width_order_is_the_same_step() {
    assert_eq!(Step::did("Ｏ２"), Step::did("O2"));
}

#[test]
fn questions_are_canonicalised_too() {
    // Ask is hashed into the leaf like Do is, so it needs the same treatment.
    assert_eq!(Step::asked(PRECOMPOSED), Step::asked(DECOMPOSED));
}

#[test]
fn ascii_orders_are_left_exactly_as_they_were() {
    // Every leaf already anchored was built from ASCII. If this changes any of it, introducing
    // canonicalisation silently invalidates runs that are already on chain.
    for s in ["give aspirin 300", "ecg", "o2 15L nrb", "adrenaline 1mg iv push"] {
        assert_eq!(Step::did(s), Step::Do(s.to_string()));
    }
}
