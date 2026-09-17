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

/// Every string literal in a source file.
///
/// One pass, because the two-pass version was wrong in a way that hid exactly what this file looks
/// for. Stripping comments first means stripping from every `//` — including the one inside
/// `"https://storage.googleapis.com/…"`, which takes the rest of that line and the string's own
/// closing quote with it. From there the scanner is out of phase: code reads as string content and
/// strings read as code, so every sentence after the first URL in the file is invisible. That is
/// how `ward_chain.rs` came to tell whoever queued a patient that "the ward will place her" with
/// this test passing on the file.
///
/// So: one walk, each thing recognised where it starts.
///   * `//` and `/* */` outside a string are comments, and block comments nest in Rust.
///   * `"…"` is a string and `\` escapes the next character; `r"…"` and `r#"…"#` escape nothing.
///   * `'x'` and `'\n'` are char literals — `'"'` is three characters of code, and reading it as a
///     string once made this test find a literal forty lines long. `'a` in `&'a str` is a lifetime,
///     an apostrophe with no partner, and skipping to the next one swallows the rest of the file.
fn literals(src: &str) -> Vec<String> {
    let c: Vec<char> = src.chars().collect();
    let at = |i: usize| c.get(i).copied();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < c.len() {
        // ── comments ────────────────────────────────────────────────────────
        if at(i) == Some('/') && at(i + 1) == Some('/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if at(i) == Some('/') && at(i + 1) == Some('*') {
            let mut depth = 1;
            i += 2;
            while i < c.len() && depth > 0 {
                if at(i) == Some('/') && at(i + 1) == Some('*') {
                    depth += 1;
                    i += 2;
                } else if at(i) == Some('*') && at(i + 1) == Some('/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        // ── raw strings: r"…", r#"…"#, br##"…"## ────────────────────────────
        if at(i) == Some('r') || (at(i) == Some('b') && at(i + 1) == Some('r')) {
            let mut j = i + if at(i) == Some('b') { 2 } else { 1 };
            let start = j;
            while at(j) == Some('#') {
                j += 1;
            }
            let hashes = j - start;
            if at(j) == Some('"') {
                j += 1;
                let close: String =
                    std::iter::once('"').chain(std::iter::repeat_n('#', hashes)).collect();
                let mut lit = String::new();
                while j < c.len() {
                    if c[j] == '"' && c[j..].iter().take(close.len()).collect::<String>() == close {
                        j += close.len();
                        break;
                    }
                    lit.push(c[j]);
                    j += 1;
                }
                out.push(lit);
                i = j;
                continue;
            }
        }
        // ── a char literal, or a lifetime ───────────────────────────────────
        if at(i) == Some('\'') {
            if at(i + 1) == Some('\\') {
                // `'\n'`, `'\''`, `'\u{2019}'` — to the closing quote, whatever is between.
                let mut j = i + 2;
                while j < c.len() && c[j] != '\'' {
                    j += 1;
                }
                i = j + 1;
            } else if at(i + 2) == Some('\'') {
                i += 3;
            } else {
                // A lifetime. The apostrophe is all there is to skip.
                i += 1;
            }
            continue;
        }
        // ── a string ────────────────────────────────────────────────────────
        if at(i) != Some('"') {
            i += 1;
            continue;
        }
        i += 1;
        let mut lit = String::new();
        while i < c.len() {
            match c[i] {
                // An escape and whatever it escapes, neither of which ends the literal.
                '\\' => i += 2,
                '"' => {
                    i += 1;
                    break;
                }
                ch => {
                    lit.push(ch);
                    i += 1;
                }
            }
        }
        out.push(lit);
    }
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

/// **The scanner has to read the whole file, and it stops at the first lifetime.**
///
/// `'a` in `fn f<'a>(…)` is an apostrophe with no partner. The scanner treats every apostrophe as
/// the start of a char literal and runs to the next one, so from the first lifetime onward it is
/// reading code as if it were inside a quote — and every string literal after that point is
/// invisible to the test above.
///
/// Which means the test has been passing on a file it stopped reading: `ward_chain.rs` tells the
/// factory to "leave the pack's case empty and the ward will place her", in a sentence sent to
/// whoever queued a patient who may be a man, and this test has never seen it.
#[test]
fn the_scanner_reads_past_a_lifetime() {
    let src = "fn f<'a>(x: &'a str) -> &'a str { let s = \"she is in here\"; s }";
    let found = literals(src);
    assert!(found.iter().any(|l| l.contains("she is in here")),
            "everything after the first lifetime is invisible to this test: {found:?}");

    // And a char literal is still a char literal, quote and all — reading `'\"'` as the start of a
    // string is the other way this scanner has desynchronised before.
    let src = "match c { '\"' => 1, '\\\\' => 2, _ => 0 } let s = \"he is in here\";";
    let found = literals(src);
    assert!(found.iter().any(|l| l.contains("he is in here")), "{found:?}");
    assert_eq!(found.len(), 1, "a char literal is not a string: {found:?}");
}

/// **The pages, and the answers about a patient, name no pronoun either.**
///
/// The rule above covers `ward.rs` and `ward_chain.rs`, which is where it started. The receipt is
/// built in `main.rs` and said "what she did" over Yonas Tesfaye for as long as it existed — the
/// scanner had simply never been pointed at it (director, 17 ก.ย., D1).
///
/// Whole-file is the wrong scope for `main.rs`: it carries the pronoun table itself, the season's
/// own episode titles, and scripted case text about patients whose sex the case wrote. So the scope
/// is the functions that build a page a stranger reads, or an answer about one patient — the places
/// that have an id and no persona, which is exactly where the rule came from.
#[test]
fn the_pages_about_a_patient_name_no_pronoun() {
    let src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"),
    )
    .expect("main.rs");

    for name in ["fn receipt_page", "fn ward_page_missing", "fn ward_chart", "fn ward_receipt",
                 "fn open_shift", "fn open_review"] {
        let i = src.find(name).unwrap_or_else(|| panic!("{name} is not in main.rs any more"));
        let (mut depth, mut started, mut end) = (0i32, false, i);
        for (k, c) in src[i..].char_indices() {
            match c {
                '{' => { depth += 1; started = true; }
                '}' => { depth -= 1; if started && depth == 0 { end = i + k + 1; break; } }
                _ => {}
            }
        }
        for lit in literals(&src[i..end]) {
            // A literal that is *only* a pronoun is a pronoun table — the page reading the
            // persona and choosing, which is the thing this rule asks for rather than the thing it
            // forbids. A pronoun among other words is a sentence.
            if ["her", "she", "hers", "his", "him", "he", "the patient", "the patient's"]
                .contains(&lit.trim().to_ascii_lowercase().as_str())
            {
                continue;
            }
            for word in ["her", "she", "hers", "his", "him", "he"] {
                let bare = lit
                    .split(|c: char| !c.is_ascii_alphabetic())
                    .any(|w| w.eq_ignore_ascii_case(word));
                assert!(!bare,
                        "{name} says {word:?} about somebody it has not met — this code has a \
                         patient id, and the page above it has a persona to ask: {lit:?}");
            }
        }
    }
}

/// **A sentence a stranger reads in order to press something is twelve words or fewer.**
///
/// The UX review of 17 ก.ย. (G3) found the pages written in long literary sentences — *"Nothing
/// here is this server's word for it"*, *"a stay that ended is not one anybody can add to"*. That
/// reads well to the engineer who wrote it and badly to a medical student in their second language
/// deciding what to do in three seconds. The rule that came out of it, and that the director asked
/// for as a test: every string a user must read to decide a press is at most twelve words.
///
/// The scan is `press_words.mjs`, run over what the server serves — the script that builds the
/// controls and the markup they sit in — because the strings live in JavaScript and JavaScript is
/// what can read them. Three collections: the words on a control, the strip's own sentences, and
/// the sentence that stands between a stranger and a control that will not answer yet.
///
/// Prose that explains rather than directs is deliberately out of scope: the receipt's account of
/// why a hash resolves to nothing is read with nothing in hand to press.
#[test]
fn every_sentence_a_stranger_presses_on_is_twelve_words_or_fewer() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = std::process::Command::new("node")
        .arg(here.join("tests/press_words.mjs"))
        .arg(here.join("static/bay.js"))
        .arg(here.join("static/bay-surface.html"))
        .arg(here.join("static/world/shift.html"))
        .arg(here.join("static/world/review.html"))
        .output()
        .expect("run node");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
