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

/// **The page draws the face the view carries, and holds no opinion about which.**
///
/// The frame on a station is the case's own authored still — the canon patient, shot for the
/// scenario. On the ward the person in that bed is somebody else, so a ward view carries her
/// portrait and the frame takes it. What must not appear here is a second copy of the ladder: no
/// state words, no sizes, no bucket, no fallback logic. One truth, on the server.
#[test]
fn the_frame_takes_the_face_the_view_sent() {
    let js = bay_js();
    let paint = body_of(&js, "paintStill");
    assert!(paint.contains("WARDFACE"),
            "the frame does not look at the face the view sent, so a ward shift wears the case's \
             own patient: {paint}");
    // The season has a ladder of its own for the stills shot per station, and it predates all of
    // this — what must not appear is a *second* one for the ward's faces. So the scope is the two
    // functions where a ward picture is chosen, plus the two things only a copy of the server's
    // rule would need: where the bucket is, and what the small sibling is called.
    for ladder in ["deteriorating", "critical", "recovered", "_256"] {
        for name in ["paintStill", "paint"] {
            let body = without_comments(&body_of(&js, name));
            assert!(!body.contains(ladder),
                    "{name} knows {ladder:?} — that is a second copy of the server's ladder, and \
                     two copies is how a picture comes to disagree with the line beside it");
        }
    }
    assert!(!js.contains("vitals-world-portraits"),
            "and the page must not know where portraits live: it draws the URL it was given, and \
             the day that bucket changes nothing in the page has to");
}

/// The mark in the bar is the mark in the tab — one drawing, inlined in two places.
#[test]
fn the_mark_in_the_bar_is_the_mark_in_the_tab() {
    let favicon = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/world/favicon.svg"),
    )
    .expect("the mark");
    let trace = favicon
        .split("points=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .expect("the trace's own path");

    let main = std::fs::read_to_string(repo().join("crates/vitals-web/src/main.rs")).expect("main.rs");
    assert!(main.contains(trace),
            "the bar's mark and the tab's mark have drifted apart — the trace in favicon.svg is \
             {trace:?} and the bar draws something else");
}

/// **At the bedside the face is the biggest thing in the patient block.**
///
/// The first attempt put it there at 2.6rem — a circle you have to look for, and the founder who
/// asked "ไม่มีรูปผู้ป่วยหรอ" would have asked again. She is a 768 px picture on the wire either
/// way, so drawing her small costs the same bytes and answers nothing.
#[test]
fn the_face_is_the_size_of_a_face() {
    let css = bay_css();
    let rule = css
        .split(".pt-face{")
        .nth(1)
        .map(|r| r.split('}').next().unwrap_or("").to_string())
        .expect("the face has a rule of its own");
    let px = |decl: &str| -> f64 {
        rule.split(decl)
            .nth(1)
            .and_then(|r| r.split(|c: char| !(c.is_ascii_digit() || c == '.')).find(|s| !s.is_empty()))
            .and_then(|n| n.parse::<f64>().ok())
            .map(|n| if rule.contains(&format!("{decl}{n}rem")) { n * 16.0 } else { n })
            .unwrap_or(0.0)
    };
    assert!(px("width:") >= 96.0, "the face is {}px wide, which is a thumbnail: {rule}", px("width:"));
    assert!(px("height:") >= 96.0, "and {}px tall", px("height:"));
    assert!(rule.contains("object-fit:cover"), "a face squeezed out of shape is worse than none: {rule}");

    // The frame's own image is a different element with a different job, and it must not be the
    // one wearing this rule — a 128 px still in the stage's frame would be a postage stamp.
    assert!(!css.contains("#fallback{width:128"), "the frame is not the face");
}

/// **The corner of the page is the mark, and there is one name for the way out.**
///
/// The founder said "มุมบนซ้าย" — the corner. The mark first went into the bay's own bar, which is
/// the page's third row; above it sat a text wordmark and a button reading "← the ward" while the
/// strip's button read "← the globe". One door with two names, and neither of them in the corner.
#[test]
fn the_corner_is_the_mark_and_the_exits_agree() {
    let page = compose_ward_page();
    let first = page.find("<a ").expect("the page has a link in it");
    let brand = &page[first..first + 400.min(page.len() - first)];
    assert!(brand.contains("href=\"/\""), "the first link on the page is the way home: {brand}");
    assert!(brand.contains("class=\"brand\""), "and it is the brand: {brand}");
    assert!(brand.contains("<svg"), "carrying the mark, not only the words: {brand}");
    assert!(!page.contains("← the ward"),
            "\"← the ward\" and \"← the globe\" point at the same door, and a door with two names \
             is two doors to a reader");
}

/// The ward's page as the server composes it, which is the only form a visitor ever sees.
fn compose_ward_page() -> String {
    let shift = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/world/shift.html"),
    )
    .expect("the shift page");
    let surface = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/bay-surface.html"),
    )
    .expect("the bay's surface");
    let main = std::fs::read_to_string(repo().join("crates/vitals-web/src/main.rs")).expect("main.rs");
    // The server's own two constants, read out of its source: a copy here would pass while the
    // page a visitor gets was something else.
    let brand = rust_concat(&main, "const WORLD_BRAND: &str = concat!(");
    let eternal = rust_literal(main.split("const ETERNAL_BRAND: &str = ").nth(1).expect("the wordmark"));
    shift.replace("<!--BAY-->", &surface.replace(&eternal, &brand)).replace("<!--BRAND-->", &brand)
}

/// The first Rust string literal in `src`, unescaped.
fn rust_literal(src: &str) -> String {
    let mut out = String::new();
    let mut chars = src[src.find('"').expect("a literal") + 1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next().unwrap_or('\\')),
            '"' => break,
            _ => out.push(c),
        }
    }
    out
}

/// Every literal in a `concat!(..)`, joined — the way rustc would.
fn rust_concat(src: &str, header: &str) -> String {
    let body = src.split(header).nth(1).expect("the concat").split("\n);").next().expect("closed");
    let mut out = String::new();
    let mut rest = body;
    while let Some(i) = rest.find('"') {
        let piece = rust_literal(&rest[i..]);
        out.push_str(&piece);
        // Past this literal: find its closing quote by walking it the same way.
        let mut n = i + 1;
        let mut chars = rest[i + 1..].chars();
        while let Some(c) = chars.next() {
            n += c.len_utf8();
            match c {
                '\\' => {
                    if let Some(e) = chars.next() {
                        n += e.len_utf8();
                    }
                }
                '"' => break,
                _ => {}
            }
        }
        rest = &rest[n..];
    }
    out
}

/// **The sheet is read before anything is done, so nothing may cover it.**
///
/// At 1460×900 the founder's own window, the PRESENTS line was clipped mid-word under the 128 px
/// face: the sheet lives inside the frame, the frame is a fixed share of a fixed-height column,
/// and the face took its bottom. A sheet a stranger reads before the first order cannot be
/// half-hidden — and the fix is not a smaller face but a sheet that is not inside a frame at all
/// while it is the thing being read.
#[test]
fn the_stem_sheet_is_never_under_the_face() {
    let css = bay_css();
    // While the sheet is showing on the ward, the frame sizes to it rather than the other way
    // round: no fixed share of the column, no absolute fill, nothing clipped.
    assert!(css.contains("html.is-ward .pt-art:has(> .stem:not(.hide))"),
            "nothing makes room for the sheet on the ward, so the face takes its bottom");
    let rule = css
        .split("html.is-ward .pt-art:has(> .stem:not(.hide))")
        .nth(1)
        .and_then(|r| r.split('}').next())
        .unwrap_or("")
        .to_string();
    assert!(rule.contains("height:auto"), "the frame takes the sheet's own height: {rule:?}");

    let stem = css
        .split("html.is-ward .pt-art > .stem:not(.hide)")
        .nth(1)
        .and_then(|r| r.split('}').next())
        .unwrap_or("")
        .to_string();
    assert!(stem.contains("position:relative"),
            "and the sheet sits in the flow rather than filling a box that is too short: {stem:?}");
    assert!(stem.contains("overflow:visible"), "with nothing cut off: {stem:?}");
}
