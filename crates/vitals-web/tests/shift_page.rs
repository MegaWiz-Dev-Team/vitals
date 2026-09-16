//! The ward's shift page, as a stranger meets it — the six things the founder found in a browser.
//!
//! He opened `/ward/1789528326` on staging on 16 ก.ย. at 1860×960 and could not reach the ask bar
//! to type: *"กด scroll ลงไปพิมพ์ไม่ได้"*. Driving the same page at the same size showed why, and
//! showed four more beside it. None of them is a server bug and none would have been caught by a
//! test of the server, which is what this file is for.
//!
//! The pure decisions are in `shift_logic.mjs`, pulled out of `bay.js` and run for real. What is
//! left here is wiring: rules that must exist in the stylesheet, and calls that must happen at the
//! moment the head is taken. Those are asserted against the source that ships, by brace matching,
//! because the alternative is asserting nothing until somebody opens a browser again.

use std::path::PathBuf;
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bay_js() -> String {
    std::fs::read_to_string(repo().join("crates/vitals-web/static/bay.js")).expect("bay.js")
}

fn bay_css() -> String {
    std::fs::read_to_string(repo().join("crates/vitals-web/static/bay.css")).expect("bay.css")
}

/// The code, with the prose taken out — block comments and line comments both.
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

/// One function out of the page, by brace matching from its header.
fn body_of(script: &str, name: &str) -> String {
    for head in [format!("function {name}("), format!("const {name}="), format!("async function {name}(")] {
        let Some(i) = script.find(&head) else { continue };
        let (mut depth, mut started) = (0i32, false);
        for (k, c) in script[i..].char_indices() {
            match c {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => {
                    depth -= 1;
                    if started && depth == 0 {
                        return script[i..i + k + 1].to_string();
                    }
                }
                _ => {}
            }
        }
    }
    panic!("{name} is not in bay.js any more — renamed, or deleted with its test left behind");
}

/// The page's own small decisions, run as JavaScript rather than described in Rust.
#[test]
fn the_shift_pages_decisions_hold() {
    let out = Command::new("node")
        .arg(repo().join("crates/vitals-web/tests/shift_logic.mjs"))
        .arg(repo().join("crates/vitals-web/static/bay.js"))
        .output()
        .expect("run node");
    assert!(
        out.status.success(),
        "the shift page's own logic failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// **The ask bar has to be reachable.**
///
/// On a desktop the bay is a cockpit: `100dvh`, no page scroll, each column scrolling inside
/// itself. The ward's shift page is that cockpit with a page header above it — so a bay given the
/// whole viewport sits 53 px lower than the viewport it was given, and the ask bar, which is the
/// one control a stranger needs, falls off the bottom of a `body` that has been told not to
/// scroll. Measured at the founder's own window: `innerHeight` 960, `scrollHeight` 1015,
/// `overflow: hidden`.
///
/// The fix is that on the ward the height comes from the column rather than from the viewport, so
/// the test is that the cockpit block says something about `html.is-ward` at all — it is the
/// release of a rule written for a page that has nothing above the bay.
#[test]
fn the_cockpit_leaves_room_for_the_wards_own_header() {
    let css = bay_css();
    let i = css.find("@media (min-width:69rem) and (min-height:40rem){").expect("the cockpit block");
    let block = &css[i..];
    let end = block.find("\n}\n").map(|e| e + 3).unwrap_or(block.len());
    let block = &block[..end];

    assert!(block.contains("body:has(#game:not(.hide)){overflow:hidden}"),
            "the cockpit still takes the page's scroll away, which is the rule the ward's header \
             has to be released from — if this moved, this whole test needs rewriting");
    assert!(block.contains("html.is-ward"),
            "the cockpit gives the bay the whole viewport and the ward's page puts a header above \
             it, so the ward has to say how tall its own bay is. Nothing here does, which is the \
             55 px the founder could not scroll to: {block}");
    assert!(block.contains("html.is-ward #game") || block.contains("html.is-ward .app"),
            "and the release has to reach the bay itself, not only the body around it");
}

/// **Taking the head is what starts the shift** — the clock and the ask bar both.
///
/// The Eternal bay opens a run and starts everything in one function. The ward's does it in two:
/// `openShift` reads her off the chain, and `takeShift` is where a stranger becomes the person
/// treating her. Everything the bay's `start` does at the bottom — the clock, the input, the send
/// button — has to happen there too, and none of it did: the founder watched the clock sit at
/// 0:00 through an exchange, and the ask bar is `disabled` in the markup and was never enabled.
#[test]
fn taking_the_shift_starts_the_clock_and_opens_the_ask_bar() {
    let js = bay_js();
    let take = body_of(&js, "takeShift");
    assert!(take.contains("run()"),
            "nothing in takeShift starts the clock, so a shift on the ward runs at 0:00 for ever: \
             {take}");
    assert!(take.contains("wardGate()"),
            "the ask bar, the send button and the chips are shut before the head is taken, and \
             `wardGate` is the one place that opens them. A ward shift that never calls it is a \
             patient you may not speak to: {take}");

    // And the gate has to be a gate: one answer, from `takeFirst`, applied to every control that
    // would touch her. A gate that only greys the chips leaves the input a stranger types into.
    let gate = body_of(&js, "wardGate");
    assert!(gate.contains("takeFirst"), "the gate has to ask the same question everything else does: {gate}");
    for control in ["#cmd", "#send", "#mic", "#chips"] {
        assert!(gate.contains(control), "{control} is a way to treat her and the gate does not reach it: {gate}");
    }
}

/// **A press that cannot do anything must not be written down.**
///
/// Before the head is taken there is no run, so `step` returns at its first line — but `doOrder`
/// has already written the order into the transcript. The founder pressed "has she had this
/// before?" and got a line at 0:00 that nobody answered, on a page whose whole promise is that
/// the transcript is what happened. Reproduced in a driven browser: one press, no network call,
/// one phantom line.
#[test]
fn nothing_is_written_to_her_chart_before_the_head_is_taken() {
    let js = bay_js();
    let fire = body_of(&js, "fire");
    assert!(fire.contains("takeFirst"),
            "fire() routes every press and is the one place that can refuse one before the head \
             is taken. It does not ask: {fire}");
}

/// **The ward's patients are not all women.**
///
/// Driving a real shift on Rafael Moreira on 16 ก.ย. put this on screen: *"shift 1 of her stay"*,
/// *"Her chart is the chain"*, *"hand her back"* — over a man. The bay has had a pronoun table
/// since the season (`pro()`, read off the same `who` string the bed label prints, defaulting to
/// neither), and the ward's own strip predates none of it; it was simply written in one voice.
///
/// So the rule is that the strip has no gendered word of its own: every one comes from `pro()` at
/// the moment it is said. Asserted over the string literals in the ward's own functions, because
/// the words are the bug and a test of the code around them would not see it.
#[test]
fn the_wards_own_sentences_take_the_patients_pronoun() {
    let js = bay_js();
    // `fire` is on the list because it is the sentence a stranger reads on *every* press before
    // the head is taken — the most-read line on the page, and it said "her" over a man until the
    // director caught it in review. The rule was right and the list was short.
    for name in ["wardBar", "openShift", "takeShift", "handBack", "handOver", "handOverInner", "fire"] {
        // Comments first: they are prose about the code, they are allowed to name a woman, and an
        // apostrophe in one ("the case's own patient") would otherwise be read as a quote and
        // shift every literal after it by one.
        let body = without_comments(&body_of(&js, name));
        for literal in body.split('\'').skip(1).step_by(2) {
            for word in ["her", "she", "Her", "She", "his", "him", "he", "His"] {
                let bare = literal
                    .split(|c: char| !c.is_ascii_alphabetic())
                    .any(|w| w == word);
                assert!(!bare,
                        "{name} says {word:?} in its own words — the ward admits men and women and \
                         the pronoun belongs to the patient, not to the sentence: {literal:?}");
            }
        }
    }
}
