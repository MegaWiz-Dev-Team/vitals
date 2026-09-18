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
    // `stemHtml` joined the list when the ward started drawing its own sheet: its footer told every
    // stranger that "what you do is on her chart" and that "she stays", over whoever was in the bed.
    for name in ["wardBar", "openShift", "takeShift", "handBack", "handOver", "handOverInner", "fire", "stemHtml", "shiftLine", "wardBed", "endWords",
                     "disarmEnd"] {
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
    compose_for_test("static/world/shift.html", true)
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

/// **There is no season ending on the ward.**
///
/// The founder's shift on Salma Gaber reached `win_discharge` two and a half simulated minutes in,
/// and the page did what the bay does at a terminal: the result panel, the sweep, the ending flow —
/// and, in his words, *"ไปใช้ vdo หน้าคนอื่นอีก"*, another person's face. The season's endings are
/// written for the season's patients. A shift is a few minutes of somebody else's stay, and what
/// it ends with is a sentence and a hand-over.
#[test]
fn the_seasons_ending_never_plays_on_the_ward() {
    let js = bay_js();
    let finish = without_comments(&body_of(&js, "finish"));
    assert!(finish.contains("WARD"),
            "finish() runs the season's ending for every run, ward or not: {finish}");

    // The ward's own ending: a sentence, the clock stopped, the controls shut. None of the
    // season's machinery, which is written for the season's patients and their faces.
    let ward = without_comments(&body_of(&js, "wardFinish"));
    for season in ["#result", "sweep(", "endFlow(", "flatline(", "settle(", "showMarks(", "showDebrief("] {
        assert!(!ward.contains(season),
                "the ward's ending reaches for {season:?}, which belongs to the season: {ward}");
    }
    assert!(ward.contains("wardSay("), "it says what happened in the strip");
    assert!(ward.contains("stop()"), "and the clock stops with the shift");
}

/// **Nothing of the season is on the ward host** (founder, 16 ก.ย.: *"ผมไม่ได้ให้เอาเคสของ vitals
/// เดิมมาใช้ใน world"*).
///
/// Not its episodes and stations as the ward's cases, not their stills or films, not the result
/// flow, not the names. The symptom was Somsri's face over Salma Gaber, but the page carried far
/// more than that: the season's whole episode list sat in the composed markup, and so did the
/// result panel the founder was shown at his discharge.
///
/// The season's entry keeps every one of them, which is the other half of the test — this is a
/// page composed per host, not a feature deleted from both.
#[test]
fn the_ward_host_carries_nothing_of_the_season() {
    let ward = compose_ward_page();
    for season in [
        "OSCE", "EP1", "EP5", "value=\"osce-", "value=\"ep1\"",
        "/img/cases", "/clip/", "id=\"result\"", "id=\"cine\"", "id=\"sweep\"",
        "← episodes", "restart",
    ] {
        assert!(!ward.contains(season),
                "the ward page still carries {season:?} — the season's, not this ward's");
    }

    // And the bay is still the bay on vitals.academy.
    let eternal = compose_for_test(PAGE_FILE, false);
    for kept in ["OSCE", "value=\"osce-", "id=\"result\"", "← episodes"] {
        assert!(eternal.contains(kept),
                "the Eternal entry lost {kept:?} — this is a page composed per host, not a feature \
                 taken from both");
    }
}

const PAGE_FILE: &str = "static/index.html";

/// The page as the server composes it for one host or the other.
fn compose_for_test(page: &str, ward: bool) -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let html = std::fs::read_to_string(dir.join(page)).expect("the page");
    let surface = std::fs::read_to_string(dir.join("static/bay-surface.html")).expect("the surface");
    let main = std::fs::read_to_string(repo().join("crates/vitals-web/src/main.rs")).expect("main.rs");
    let brand = rust_concat(&main, "const WORLD_BRAND: &str = concat!(");
    let eternal = rust_literal(main.split("const ETERNAL_BRAND: &str = ").nth(1).expect("the wordmark"));
    let open = rust_literal(main.split("const SEASON_ONLY_OPEN: &str = ").nth(1).expect("the marker"));
    let close = rust_literal(main.split("const SEASON_ONLY_CLOSE: &str = ").nth(1).expect("the marker"));

    let surface = if ward {
        let mut out = String::new();
        let mut rest = surface.as_str();
        while let Some(i) = rest.find(&open) {
            out.push_str(&rest[..i]);
            rest = match rest[i..].find(&close) {
                Some(e) => &rest[i + e + close.len()..],
                None => "",
            };
        }
        out.push_str(rest);
        out.replace(&eternal, &brand)
    } else {
        surface.replace(&open, "").replace(&close, "")
    };
    html.replace("<!--BAY-->", &surface)
        .replace("<!--BRAND-->", if ward { &brand } else { "" })
}

/// **The bay parses as one script.**
///
/// `page.rs` scans a page's own inline block for the edit that left half of itself behind. The
/// ward's page has no inline block — every line of its logic is in `bay.js` — and that file defeats
/// the brace scanner, which was written for a page. So it is handed to node, which is the thing
/// that has to parse it in the end: a syntax error anywhere in `bay.js` takes the whole bay down
/// on both hosts, silently, with the markup still looking perfectly fine.
#[test]
fn the_bay_parses_as_one_script() {
    let bay = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/bay.js");
    let out = Command::new("node")
        .arg("-e")
        .arg("new Function(require('fs').readFileSync(process.argv[1], 'utf8'))")
        .arg(&bay)
        .output()
        .expect("run node");
    assert!(out.status.success(),
            "bay.js does not parse, so nothing on either host runs:\n{}",
            String::from_utf8_lossy(&out.stderr));
}

/// **On the ward `over` closes treatment, never the hand-over.**
///
/// The director took a patient whose case had already reached discharge from the idle replay: the
/// strip said *"is ready to go home. Hand over to close her stay on chain"*, and pressing hand over
/// did nothing at all — two presses, sixty seconds, no request. `wardFinish` sets `over` and then
/// enables the one control it leaves open, and both `endRun` and the button's own handler begin
/// `if(!id||over)return`.
///
/// The consequence is the whole discharge path: the stranger who got her well cannot close her,
/// the next stranger inherits a finished state and cannot either, and only the ticker's death ever
/// closes a patient. The founder's "หายแล้วกลับบ้าน" ending was unreachable on a public ward.
#[test]
fn a_finished_shift_can_still_be_handed_over() {
    let js = bay_js();
    let run = without_comments(&body_of(&js, "endRun"));
    assert!(!run.contains("if(!id||over)return;"),
            "endRun refuses a finished shift, and on the ward the finished shift is exactly the \
             one with something left to do: {run}");
    // The guard that remains has to let the ward through, and the season's bay keep its own
    // behaviour — a finished station is finished, and its button is disabled anyway.
    let end = without_comments(&body_of(&js, "endRun"));
    assert!(end.contains("WARD"), "the ward is what the exception is for: {end}");
    // And the press reaches it without arming: there is nothing left to lose on a shift the
    // engine has already ended, and a second press to confirm is a second chance to do nothing.
    let ward = without_comments(&body_of(&js, "wardFinish"));
    assert!(ward.contains("armed") || ward.contains("disarmEnd"),
            "the finished shift's button must not be sitting armed from before: {ward}");
}

/// **On the ward, every word about the case comes off the wire.**
///
/// The founder opened a World-case patient and read EP1's name over her, EP1's six questions under
/// her, and no title at all. `bay.js` ships to both hosts, so the season's table is in the file on
/// the ward too — and the ward's opener asked it which case this was. A compiled case is not in it,
/// so `SEASON.find(…)||SEASON[0]` answered EP1 and the whole page believed it: the header, the
/// sheet, the tray, the pronouns, the monitor's alarm limits.
///
/// So the opener reads the payload and nothing else. A patient whose case the payload could not
/// describe gets a sentence saying so — never another patient's name, which is the failure this
/// replaces and the only one that can put a wrong drug under a real hand.
#[test]
fn a_world_case_is_drawn_from_the_payload_and_never_from_the_season() {
    let js = bay_js();
    let open = without_comments(&body_of(&js, "openShift"));

    assert!(open.contains("wardCard("),
            "the ward's opener does not build its card from the payload: {open}");
    assert!(open.contains("content"),
            "and nothing in it reads the case content the payload carries: {open}");
    assert!(!open.contains("SEASON"),
            "the opener still consults the page's table of the season's sixteen, which on the ward \
             host is a table of sixteen patients who are not in this bed: {open}");
    // The sentence changed on 18 September and the rule did not: a case the payload cannot describe
    // is a sentence, and now it is the true one. "Not on this page yet" was printed *after* a
    // stranger pressed "take a shift" on the board, and said nothing about why or what to do; the
    // board refuses the offer now, and this page says what happened and shows the chart.
    assert!(open.contains("cannot be opened"),
            "a case the payload could not describe has to be a sentence: {open}");
    assert!(open.contains("chartPage("),
            "and the sentence rides her own chart — who she is, and every shift that treated her, \
             which is the one thing this page can still honestly show: {open}");

    // One place answers "which case is this", and on a shift the answer is the ward's card. The
    // season's shelf keeps its own lookup — this is a page that serves two hosts, not a feature
    // taken from one of them.
    let src = without_comments(&js);
    assert!(src.contains("const ep=()=>epOf("),
            "the page still falls through to the shelf's first entry, which is how EP1 got over \
             another patient");

    // The tray is the case's own, by the same rule: `CHIPS` is the season's table of questions.
    let chips = without_comments(&body_of(&js, "renderChips"));
    assert!(chips.contains("WARDCHIPS") || chips.contains("chipRows("),
            "the quick questions still come from the season's table: {chips}");
}

/// **Before the payload lands, the ward's page names nobody.**
///
/// The director opened a shift on staging throttled to 1.5 Mbit and watched `#pt-name` read
/// "Ing · F 19" for about a second before the payload arrived and the card was rebuilt. It is the
/// bay's own initial markup — EP1's patient, written into the HTML so the bay paints something
/// before its first view — and on a shift page it is another patient's name over this bed. A
/// screenshot at 800 ms is the same error as a screenshot at eight seconds; it is *the* error this
/// whole batch of work is about, arriving through the one door nothing had closed.
///
/// So on the ward host the markup carries no patient at all and the card fills from the payload.
/// The Eternal page keeps its defaults: this is a page composed per host, not a default deleted
/// from both.
#[test]
fn the_wards_first_paint_carries_no_patient() {
    let ward = compose_ward_page();

    // The season's own people, read out of the page's table so this test cannot go stale when the
    // shelf changes. A `who` is "Name · SEX AGE" and a `line` is what that patient presents with —
    // both distinctive enough to match whole, which is the point: a bare "Ing" is a substring of
    // half the English on the page.
    let js = bay_js();
    let mut people = Vec::new();
    for (field, src) in [("who:'", &js), ("line:'", &js)] {
        for piece in src.split(field).skip(1) {
            if let Some(v) = piece.split('\'').next() {
                if !v.is_empty() {
                    people.push(v.to_string());
                }
            }
        }
    }
    assert!(people.len() > 10, "the season's table is where these come from: {people:?}");
    for who in &people {
        assert!(!ward.contains(who.as_str()),
                "the ward's markup carries the season's {who:?} before a single byte of payload \
                 has arrived");
    }

    // And the two elements that carried them are empty rather than gone: the card writes into
    // them the moment the payload lands.
    let inner = |html: &str, id: &str| -> String {
        html.split(&format!("id=\"{id}\""))
            .nth(1)
            .and_then(|r| r.split_once('>'))
            .and_then(|(_, r)| r.split_once('<'))
            .map(|(t, _)| t.to_string())
            .unwrap_or_else(|| panic!("no #{id} on the page"))
    };
    for id in ["pt-name", "pt-line"] {
        assert!(inner(&ward, id).trim().is_empty(),
                "#{id} opens with {:?} over a patient nobody has read yet", inner(&ward, id));
        assert!(!inner(&compose_for_test(PAGE_FILE, false), id).trim().is_empty(),
                "and the bay keeps its own first paint — this is composed per host, not deleted");
    }
}

/// **The words a learner works in are big enough to work in.**
///
/// Measured in a browser at 1460×900 on 16 ก.ย.: the question chips 11.8 px, the kit's mode buttons
/// 11.5 px, the ask bar 12.8 px, and the sheet's own labels — PATIENT, PRESENTS — 9.1 px.
///
/// Those are the three surfaces somebody actually reads while treating a patient: what they can
/// ask, what they can type, and the sheet they read before the clock starts. A headline at 22 px
/// over a tray at 11.8 px is a page that looks designed and reads badly.
///
/// The prompt to go and measure came from a medical student's first user test — *"อยากให้ตัวอักษร
/// ใหญ่กว่านี้จะดีมากค่า"* — but she was testing **embla**, not this page, and the first version of
/// this test said otherwise. She has never seen this bay. What is left is the measurement, which is
/// this page's own and stands on its own; no user of this page has asked for it.
///
/// So this is a floor rather than a type scale: the chips, the kit, the ask bar and the sheet's
/// labels do not go below it. Asserted in the stylesheet rather than in a browser because it is the
/// stylesheet that decides — and because `rem` here is 16 px, set on `:root` and unchanged.
#[test]
fn the_words_a_learner_reads_are_big_enough() {
    let css = bay_css();
    // The font-size a selector sets, in px, reading `rem` as 16 and taking the rule's own value.
    let size = |selector: &str| -> f64 {
        // Every rule this selector opens, not the first text that happens to contain it: `#cmd{`
        // matches inside `.acts.order-mode #cmd{`, and reading that rule asks a question about a
        // border colour. The one that sets the size is the one wanted.
        let rule = css
            .match_indices(&format!("{selector}{{"))
            .filter(|(i, _)| {
                *i == 0 || matches!(css[..*i].chars().next_back(), Some('}') | Some('\n') | Some(' '))
            })
            .map(|(i, m)| css[i + m.len()..].split('}').next().unwrap_or("").to_string())
            .find(|r| r.contains("font-size:"))
            .unwrap_or_else(|| panic!("{selector} sets no font-size of its own any more"));
        // `clamp(1rem,2.6vh,1.42rem)` is read at its smallest: the floor is about the worst case,
        // and on a short window that is what the headline actually is.
        let raw = rule.split("font-size:").nth(1).expect("a size").trim_start();
        let raw = raw.strip_prefix("clamp(").unwrap_or(raw);
        let digits: String = raw.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let n: f64 = digits.parse().unwrap_or_else(|_| panic!("{selector}: {raw}"));
        // `.74rem` is 11.84 px and `.74` of nothing is not a size — the unit is read off the source
        // rather than assumed, because assuming px here would have passed this test on a page that
        // sets rem.
        if raw[digits.len()..].starts_with("rem") { n * 16.0 } else { n }
    };

    for (selector, floor, what) in [
        (".chips .btn", 13.5, "the questions, the drugs and the differential — the tray"),
        (".acts.order-mode .chips .btn", 13.0, "the same tray in order mode"),
        (".modes .btn", 13.0, "the kit's own rows"),
        ("#cmd", 14.0, "the line a stranger types into"),
        (".stem-g dt", 10.5, "PATIENT, PRESENTS — the sheet's labels"),
        (".stem-f", 10.5, "the note at the foot of the sheet"),
    ] {
        let px = size(selector);
        assert!(px >= floor,
                "{selector} is {px}px — {what}, and this page's own measurement put that below \
                 what somebody can work in");
    }

    // And the sheet stays a sheet: even at its smallest the headline leads everything the floor
    // above raised, so this is a floor and not a flattening.
    assert!(size(".stem-t") >= 16.0, "the headline leads the sheet: {}px", size(".stem-t"));
}

/// **The line you type into is on the screen, whatever the case brought with it.**
///
/// The stage on a shift page is one scrolling column: the patient block, the transcript and the
/// action bar all live in it. The season's cases bring six chips to a row and it fits. A compiled
/// case brings its own tray — this ward's typhoid case has twenty questions, twenty-five
/// investigations and nineteen treatments — and the ask bar went to the bottom of a scroll with no
/// visible end. Measured at 1280×800 before the fix: the ask bar's top edge at 975 px in an 800 px
/// window, and the body told not to scroll.
///
/// That is the founder's own *"กด scroll ลงไปพิมพ์ไม่ได้"* arriving through a second door, six days
/// after the first one was closed, and it was there before the type was raised — raising it moved
/// the bar 62 px further down, which is how it was found.
#[test]
fn the_ask_bar_is_pinned_whatever_the_case_brought() {
    let css = bay_css();
    let i = css.find("@media (min-width:69rem) and (min-height:40rem){").expect("the cockpit block");
    let block = &css[i..];
    let end = block.find("\n}\n").map(|e| e + 3).unwrap_or(block.len());
    let block = &block[..end];

    assert!(block.contains("html.is-ward .acts{position:sticky"),
            "the action bar is not pinned to the foot of the stage, so a long tray puts the ask bar \
             below a fold that cannot be scrolled to: {block}");
    let chips = block
        .split("html.is-ward .chips{")
        .nth(1)
        .and_then(|r| r.split('}').next())
        .unwrap_or("")
        .to_string();
    assert!(chips.contains("max-height"),
            "and the tray is uncapped, so it takes the stage with it: {chips:?}");
    assert!(chips.contains("overflow-y:auto"),
            "a capped tray that does not scroll is a tray with its last drug hidden: {chips:?}");
}

/// **Before the head is taken, the page is a chart to read.**
///
/// From the director's UX pass on staging, 17 ก.ย.: a stranger who has not taken the shift is shown
/// a red "⚠ nothing given yet", a NEWS2 banner reading HIGH RISK · emergency response, and a clock
/// at 0:00. Every one of those is about an action that has not started — they are true of a shift
/// in progress and are alarms about nothing before one. The page's own words already say what to do
/// ("take the shift to treat her"); an alarm beside them says the opposite, that something is wrong
/// and being ignored.
///
/// A review run is not in this state: its run is live from the first paint, so `untaken` is never
/// on it and the monitor reads as it does at any bedside.
#[test]
fn nothing_alarms_before_the_shift_is_taken() {
    let css = bay_css();
    for hidden in ["#nudge", "#newsbox", "#clock"] {
        let rule = css
            .split("html.is-ward.untaken")
            .any(|r| r.split('{').next().unwrap_or("").contains(hidden.trim_start_matches('#'))
                     || r.starts_with(&format!(" {hidden}"))
                     || r.starts_with(&format!(",{hidden}")));
        assert!(rule || css.contains(&format!("html.is-ward.untaken {hidden}")),
                "{hidden} is shown to somebody who has not taken the shift, and it is an alarm \
                 about an action that has not started");
    }

    // And the empty-chart nudge is not even written before the head is taken: it is a cue for
    // somebody treating a patient, not for somebody reading about one.
    let js = bay_js();
    let paint = without_comments(&body_of(&js, "paint"));
    let nudge = paint.split("#nudge").nth(1).unwrap_or("").split(';').next().unwrap_or("").to_string();
    let asked = paint
        .split('\n')
        .filter(|l| l.contains("takeFirst"))
        .filter_map(|l| l.split_once("const ").and_then(|(_, r)| r.split_once('=')))
        .map(|(name, _)| name.trim().to_string())
        .find(|name| nudge.contains(name.as_str()));
    assert!(asked.is_some(),
            "the nudge does not ask whether this shift has been taken — it is drawn for somebody \
             who may not give anything yet: {nudge}");
}

/// **The practice controls are the bay's, and a shift is not practice.**
///
/// "hold" stops the clock and "easy" picks how fast the patient deteriorates. Both are right for a
/// learner practising alone and neither is a thing anybody does to a patient on a public ward: the
/// clock is a lease on the chain and the deterioration is what the case says it is. They sat in the
/// ward's bar until 17 ก.ย., beside a LIVE pill and a second copy of the wordmark that the page's
/// own corner already carries.
///
/// A **review run** keeps them: it is practice, on nobody, and pausing to read is the whole point.
/// So this is a rule about a shift rather than about the host.
#[test]
fn a_shift_does_not_carry_the_practice_controls() {
    let css = bay_css();
    for control in ["#pause", "#diff"] {
        assert!(css.contains(&format!("html.is-ward:not(.is-review) {control}"))
                    || css.contains(&format!("{control},")) && css.contains("is-review"),
                "{control} is offered on a shift: the clock is a lease and the deterioration is \
                 the case's");
    }
    assert!(css.contains("html.is-ward .bar .brand"),
            "the wordmark is in the page's corner and in the bay's bar, so a shift page says it \
             twice");

    // The LIVE pill is the bay's own word for a run in progress. On a shift the strip says what is
    // happening in words a stranger reads, and the pill is one more thing to decode.
    let js = bay_js();
    let paint = without_comments(&body_of(&js, "paint"));
    assert!(paint.contains("WARDSURFACE?''") || paint.contains("!WARDSURFACE"),
            "the LIVE pill is still drawn on the ward host: {paint}");
}

/// **The quiet exit is quiet, and its question survives a resting pointer.**
///
/// C4's other half is weight. "Leave without recording" sits next to the one action this page is
/// for, so it carries no fill and no bold — a stranger tells the two apart before reading either.
/// Measured in Chrome at 1460×900 and 390×844: 400 against the primary's 700, transparent against
/// its filled card.
///
/// The armed rule comes *after* the hover rule deliberately, and that is the assertion. Both are
/// `.btn.quiet` plus one more class or pseudo-class, so they are equally specific and the later one
/// wins — and the pointer is always resting on the button at the moment it asks the question,
/// because the press is what armed it. Written the other way round, the driven page showed
/// "press again to leave" in the resting ink: the one state where the colour is the warning, in the
/// one situation where it is always hovered.
#[test]
fn the_quiet_exit_keeps_its_warning_under_the_pointer() {
    let css = bay_css();
    let quiet = css.find(".btn.quiet{").expect("the quiet button's own rule");
    let hover = css.find(".btn.quiet:hover").expect("its hover rule");
    let armed = css.find(".btn.quiet.armed").expect("the armed rule");

    let block = &css[quiet..css[quiet..].find('}').map(|e| quiet + e).unwrap_or(css.len())];
    assert!(block.contains("background:transparent"),
            "the exit that records nothing carries no fill: {block}");
    assert!(block.contains("font-weight:400"),
            "and no bold — the weight is the first thing a stranger reads: {block}");

    assert!(armed > hover,
            "the armed rule has to come after the hover rule, or the question is painted in the \
             resting colours at exactly the moment it is asked — the press is what armed it, so \
             the pointer is on the button");
    let armed_block = &css[armed..css[armed..].find('}').map(|e| armed + e).unwrap_or(css.len())];
    assert!(armed_block.contains("hover"),
            "and it has to name the hover state itself, because the pointer is resting there: \
             {armed_block}");
}

/// **Every path the page beacons to is a path the server answers with POST.**
///
/// `navigator.sendBeacon` always sends a POST. The page's exit posted a signed release to
/// `/api/ward/submit`, which this server answers on GET and nothing else — so the request never
/// matched a route, and the bed a stranger walked away from stayed held until the lease ran out.
/// Two runs on staging on 17 ก.ย. (close the tab; navigate away) both left the board reading
/// `on_shift` a minute later, and the founder had met the same thing the day before.
///
/// It is not a bug either file can see on its own: the page is right about what it sends and the
/// server is right about what it answers, and the two are about different verbs. So the test is
/// the one that spans them — every beacon target in the page, matched against the methods the
/// router really has for that path.
#[test]
fn a_beacon_only_ever_posts_to_a_route_that_answers_post() {
    let js = without_comments(&bay_js());
    let server = std::fs::read_to_string(repo().join("crates/vitals-web/src/main.rs")).expect("main.rs");

    let mut beacons: Vec<String> = Vec::new();
    for (i, _) in js.match_indices("navigator.sendBeacon(") {
        let rest = &js[i + "navigator.sendBeacon(".len()..];
        let quote = rest.chars().next().expect("a beacon with an argument");
        assert!(quote == '\'' || quote == '"' || quote == '`',
                "a beacon target built some other way — this test reads literals: {}", &rest[..40.min(rest.len())]);
        let end = rest[1..].find(quote).expect("the target's closing quote") + 1;
        let target = &rest[1..end];
        // The path is what routes; everything after the first `?` or `'+` is the query it carries.
        let path = target.split(['?', '\'']).next().unwrap_or("").trim().to_string();
        assert!(path.starts_with('/'), "a beacon to something that is not a path: {target}");
        beacons.push(path);
    }
    assert!(beacons.len() >= 2,
            "the page beacons on the way out and while it holds a head — found {}: {beacons:?}",
            beacons.len());

    for path in &beacons {
        let post = format!("(Method::Post, \"{path}\")");
        assert!(server.contains(&post),
                "the page beacons to {path} and the server has no POST route for it — a beacon is \
                 always a POST, so this request lands nowhere and the page has no way to find out");
    }
}

/// **The bar pinned over the transcript has its height reserved below it.**
///
/// The demo capture, 18 ก.ย., at 1440×810: the action bar covers the last 269 px of the transcript
/// at every scroll position — 278 px at 1280×800 — because the bar is `position: sticky` at the
/// foot of the stage and the transcript reserves 14 px for something 184 px tall. Whatever was
/// last said at the bedside is behind the thing you type into.
///
/// The height cannot be a constant in the stylesheet: the bar grows as the tray wraps and as the
/// chips open, and every constant would be right until somebody pressed "+ N more". So the page
/// measures the bar and the stage reserves exactly that — `--actsh`, kept current by a
/// `ResizeObserver`, which is the only way the reservation stays true while the bar changes shape.
///
/// What this test can hold is that both halves exist and agree on the variable's name. The
/// geometry itself is driven in a browser at both of the founder's sizes (`barcheck2.py`), because
/// no repository test here can lay out a page.
#[test]
fn the_stage_reserves_the_action_bars_own_height() {
    let js = without_comments(&bay_js());
    let css = bay_css();

    assert!(js.contains("--actsh"),
            "the page has to measure the bar: no stylesheet can know how tall it is right now");
    assert!(js.contains("ResizeObserver"),
            "and keep measuring it — the bar changes height when the tray wraps or the chips open");
    let setter = body_of(&js, "reserveTheBar");
    assert!(setter.contains("setProperty") && setter.contains("getBoundingClientRect"),
            "the reservation is the bar's measured height, not a guess: {setter}");

    assert!(css.contains("var(--actsh"),
            "and the stylesheet has to spend it, or the measurement is a number nobody reads");
    // The rule has to be inside the ward's own cockpit, where the bar is sticky. On the Eternal
    // entry the stage scrolls as a page and nothing is pinned over anything.
    let pinned = css.find("html.is-ward .acts{position:sticky").expect("the ward's sticky bar");
    let spent = css.find("var(--actsh").expect("the reservation");
    assert!(spent > pinned.saturating_sub(4000) && spent < pinned + 4000,
            "the reservation belongs beside the rule that pins the bar, so the next reader of one \
             meets the other");
}

/// **Hand over is pressed once, and stays pressed.**
///
/// Two buttons do this one act — the bedside card's (`#wardprimary`, under the face, where the
/// founder looked for it) and the strip's (`#endrun`) — and only the strip's was ever disabled.
/// So the bedside one stayed live through the anchor, and a second press sent the same leaf at a
/// head we had just moved ourselves: the chain refused it, correctly, and the page showed that
/// refusal to somebody whose shift was already on the chain.
///
/// The latch therefore belongs to the shift and not to either button: one function disables both,
/// and the repaint that redraws the card reads it rather than resetting it.
#[test]
fn the_hand_over_is_latched_by_the_shift_and_not_by_its_button() {
    let js = bay_js();

    let latch = without_comments(&body_of(&js, "handingOver"));
    for b in ["#endrun", "#wardprimary"] {
        assert!(latch.contains(b),
                "both buttons that hand a shift over have to go down together, and {b} is not in \
                 the latch: {latch}");
    }

    // The hand-over latches before it does anything else: the press that follows it is the one
    // being defended against, and it arrives while the first is still awaiting the chain.
    let inner = without_comments(&body_of(&js, "handOverInner"));
    assert!(inner.contains("handingOver(true)"),
            "the hand-over has to latch the shift, not just its own button: {inner}");

    // And the card's repaint spends the latch rather than clearing it. `paintPrimary` runs on
    // every ward poll, so a repaint that re-enabled the button would hand the second press back.
    let paint = without_comments(&body_of(&js, "paintPrimary"));
    assert!(paint.contains("disabled=HANDING") || paint.contains("disabled = HANDING"),
            "a repaint must not unlatch a shift that is being handed over: {paint}");
    assert!(paint.contains("pressPrimary"),
            "and the press is the decision pressPrimary makes, so the test of it is the test of \
             the button: {paint}");
}
