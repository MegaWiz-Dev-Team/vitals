//! Learner input has to mean the same thing whatever keyboard produced it.
//!
//! Matching is `lowercased.contains(keyword)`, which quietly assumes the text arrived as plain
//! ASCII. Every market so far has obliged. Japanese, Korean and Chinese do not: an IME emits
//! full-width Ｏ２ where a US keyboard emits O2, and Hangul reaches the server as either one
//! precomposed syllable or three jamo depending on the device. Both differences are invisible on
//! screen and total to `contains` — and, worse, to the hash, since the tape stores the text.

use vitals_sce::text::canon;

#[test]
fn a_full_width_order_is_the_same_order() {
    // What a Japanese IME produces when the learner types an oxygen order.
    assert_eq!(canon("Ｏ２ ＮＲＢ").to_lowercase(), "o2 nrb");
    assert_eq!(canon("１２誘導心電図"), "12誘導心電図");
}

#[test]
fn hangul_means_the_same_thing_from_any_device() {
    // Precomposed (what most keyboards send) and decomposed (what macOS filesystems and some
    // Android IMEs send). Identical on screen, different bytes, different hash.
    let precomposed = "\u{C544}\u{C2A4}\u{D53C}\u{B9B0}";
    let decomposed = "\u{110B}\u{1161}\u{1109}\u{1173}\u{1111}\u{1175}\u{1105}\u{1175}\u{11AB}";
    assert_ne!(precomposed, decomposed, "the test premise: these differ as bytes");
    assert_eq!(canon(precomposed), canon(decomposed));
}

#[test]
fn every_tape_ever_recorded_still_hashes_the_same() {
    // The zero-migration claim. Canonicalisation must be the identity on the ASCII that every
    // anchored run so far contains, or introducing it invalidates leaves already on chain.
    for s in [
        "give aspirin 300",
        "ecg",
        "o2 15L nrb",
        "12-lead",
        "adrenaline 1mg iv push",
        "remove o2",
    ] {
        assert_eq!(canon(s), s, "{s:?} changed");
    }
}

#[test]
fn canonicalising_twice_changes_nothing() {
    for s in ["Ｏ２", "아스피린", "心電図", "aspirin"] {
        assert_eq!(canon(&canon(s)), canon(s));
    }
}

#[test]
fn thai_and_indonesian_survive_unharmed() {
    for s in ["ให้แอสไพริน", "beri aspirin 300 mg"] {
        assert_eq!(canon(s), s);
    }
}
