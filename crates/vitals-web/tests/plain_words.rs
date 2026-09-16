//! **The server does not know who is in the bed, so it does not guess.**
//!
//! The ward's refusals and notes were written in one voice: *"she has left the ward"*, *"her chart
//! cannot be rebuilt"*, *"the ward could not take her head to close her"*. The ward admits men and
//! women — the page learned that on 16 ก.ย. and reads the pronoun off the patient — but the server
//! mostly cannot: these sentences are produced deep in code that has a patient id and no persona,
//! and the ones that reach a screen reach it as a refusal somebody is standing at a bed reading.
//!
//! So the rule here is the neutral one: **the server's own sentences use no pronoun at all.** "The
//! chart", "this patient", "the shift". Where a name or a sex is genuinely known — the strip, the
//! card, the page — the page says it, and that is tested in `shift_page.rs`.

use std::path::PathBuf;

/// Every string literal in a source file, with the prose stripped out first.
fn literals(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let code = without_comments(src);
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut lit = String::new();
        while let Some(c) = chars.next() {
            match c {
                // An escape and whatever it escapes, neither of which is the end of the literal.
                '\\' => {
                    chars.next();
                }
                '"' => break,
                _ => lit.push(c),
            }
        }
        out.push(lit);
    }
    out
}

fn without_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(i) = rest.find("/*").into_iter().chain(rest.find("//")).min() {
        out.push_str(&rest[..i]);
        rest = if rest[i..].starts_with("/*") {
            rest[i..].find("*/").map(|e| &rest[i + e + 2..]).unwrap_or("")
        } else {
            rest[i..].find('\n').map(|e| &rest[i + e..]).unwrap_or("")
        };
    }
    out.push_str(rest);
    out
}

#[test]
fn the_wards_own_sentences_name_no_pronoun() {
    for file in ["src/ward.rs", "src/ward_chain.rs"] {
        let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(file))
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        for lit in literals(&src) {
            for word in ["her", "she", "hers", "his", "him", "he"] {
                let bare = lit
                    .split(|c: char| !c.is_ascii_alphabetic())
                    .any(|w| w.eq_ignore_ascii_case(word));
                assert!(!bare,
                        "{file} says {word:?} to somebody it has never met — the ward admits men \
                         and women and this code has a patient id, not a persona: {lit:?}");
            }
        }
    }
}
