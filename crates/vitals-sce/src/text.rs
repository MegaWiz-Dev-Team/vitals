//! Canonicalising learner input.
//!
//! Everything downstream — the keyword matcher, and the hash that makes a run verifiable —
//! compares text byte by byte. That is safe only if the same intent always produces the same
//! bytes, which is true for a US keyboard and false almost everywhere else:
//!
//!   - a Japanese or Korean IME emits full-width `Ｏ２`, and `"Ｏ２".contains("o2")` is false;
//!   - Hangul arrives precomposed from most keyboards and decomposed from macOS and some Android
//!     IMEs — one syllable or three jamo, identical on screen, different bytes;
//!   - `²`, `℃` and `㎎` are single code points a clinician can and does type.
//!
//! NFKC settles all three. It is the compatibility form, so it folds the width and the ligature
//! distinctions that carry no clinical meaning, and it composes Hangul back into syllables.
//!
//! What it deliberately does not do is unify Simplified and Traditional Chinese: `心电图` and
//! `心電圖` are different characters, not different encodings of one. That difference is real —
//! it separates mainland practice from Taiwan and Hong Kong — and belongs in the language pack,
//! not in a normalisation pass that would erase it.

use unicode_normalization::UnicodeNormalization;

/// Put text in the one form the matcher and the hash agree on.
///
/// The identity on ASCII, which is what makes this safe to introduce: every tape already
/// anchored contains ASCII, so every leaf already on chain still verifies unchanged.
pub fn canon(s: &str) -> String {
    s.nfkc().collect()
}

/// Canonicalised and lower-cased: the form a keyword match compares on.
///
/// One function so every keyword matcher in the engine agrees — intervention keywords in the
/// runtime and dialogue keywords in the reveal gate. Two copies of this would drift, and a
/// question that earns a fact in one place and not the other is a bug nobody could see.
pub fn fold(s: &str) -> String {
    canon(s).to_lowercase()
}
