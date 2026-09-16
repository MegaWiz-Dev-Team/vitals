//! `f` or `m`: the two letters the persona pool spells a sex with, and the pronouns a sentence
//! about a patient takes from them.
//!
//! The ward's case door spells its patients' sex `male` / `female`; that word is read into one of
//! these two letters in exactly one place, [`crate::cases::sex_of`], and nowhere else.

/// `f` or `m`, the two letters the persona pool uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    F,
    M,
}

impl Sex {
    /// One letter in either case, with the space around it forgiven. Nothing else: a word like
    /// "female" is a rendering, and rendering is not something to pattern-match a patient out of.
    pub fn parse(s: &str) -> Option<Sex> {
        match s.trim() {
            "f" | "F" => Some(Sex::F),
            "m" | "M" => Some(Sex::M),
            _ => None,
        }
    }

    /// The pool's own letter.
    pub fn letter(self) -> &'static str {
        match self {
            Sex::F => "f",
            Sex::M => "m",
        }
    }

    /// The possessive for a sentence about the patient: "her face", "his face".
    pub fn possessive(self) -> &'static str {
        match self {
            Sex::F => "her",
            Sex::M => "his",
        }
    }

    /// The object pronoun: "no pack for her", "no pack for him".
    pub fn object(self) -> &'static str {
        match self {
            Sex::F => "her",
            Sex::M => "him",
        }
    }

    /// The possessive from the pool's letter, for a ledger entry that stores the letter; "their"
    /// when the letter is neither.
    pub fn possessive_of(letter: &str) -> &'static str {
        match Sex::parse(letter) {
            Some(s) => s.possessive(),
            None => "their",
        }
    }

    /// The word the portrait prompt uses.
    pub fn word(self) -> &'static str {
        match self {
            Sex::F => "woman",
            Sex::M => "man",
        }
    }
}
