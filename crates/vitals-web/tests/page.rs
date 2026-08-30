//! The page is one file of hand-written HTML and JavaScript, and nothing type-checks it.
//!
//! Every UI failure in this repo so far has been one of three things, and all three leave the
//! markup looking perfectly fine while the whole script dies at parse time — so the page renders,
//! and then nothing works:
//!
//!   * a `const` shadowing a name the page already uses
//!   * an edit that replaced a block but left the tail of the old one behind
//!   * a handler bound to an id that no longer exists
//!
//! These are cheap to catch mechanically and expensive to catch by hand, which is the whole
//! argument for having them here.

use std::collections::HashMap;
use std::path::PathBuf;

fn static_page(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn page() -> String {
    static_page("index.html")
}

/// The reviewer's form. Hand-written HTML and JavaScript like the bay's, served by the same
/// binary, and read by two people who cannot be asked to open a console when it does not work.
fn review_page() -> String {
    static_page("review.html")
}

/// Every hand-written page this binary serves that carries a script, by the name it is served
/// under. A page added here inherits the three checks below and costs nothing else.
fn scripted_pages() -> Vec<(&'static str, String)> {
    vec![("index.html", page()), ("review.html", review_page())]
}

/// Everything between the first `<script>` and its `</script>`.
fn script(html: &str) -> &str {
    let a = html.find("<script>").expect("no <script>") + "<script>".len();
    let b = html[a..].find("</script>").expect("unterminated <script>") + a;
    &html[a..b]
}

/// Strip strings, template literals and comments so a scan sees code and not prose.
///
/// Braces inside `${...}` still count — they are code — but a `}` inside a quoted string is not
/// an unbalanced brace, and the page is full of HTML fragments in template literals.
fn code_only(js: &str) -> String {
    let b: Vec<char> = js.chars().collect();
    let mut out = String::with_capacity(js.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            '/' if i + 1 < b.len() && b[i + 1] == '/' => {
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
            }
            '/' if i + 1 < b.len() && b[i + 1] == '*' => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            '\'' | '"' => {
                let q = c;
                i += 1;
                while i < b.len() && b[i] != q {
                    if b[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
                out.push('"');
            }
            '`' => {
                i += 1;
                // One entry per open `${`, each counting the braces nested inside it. A flat
                // counter reads `||{}` as closing the interpolation, which walked the scan off
                // the rails and reported a balanced file as broken.
                let mut interp: Vec<usize> = Vec::new();
                while i < b.len() {
                    match b[i] {
                        '\\' => i += 1,
                        '$' if i + 1 < b.len() && b[i + 1] == '{' => {
                            interp.push(0);
                            out.push('{');
                            i += 1;
                        }
                        '{' if !interp.is_empty() => {
                            *interp.last_mut().expect("non-empty") += 1;
                            out.push('{');
                        }
                        '}' if !interp.is_empty() => {
                            let d = interp.last_mut().expect("non-empty");
                            if *d == 0 {
                                interp.pop();
                            } else {
                                *d -= 1;
                            }
                            out.push('}');
                        }
                        '`' if interp.is_empty() => break,
                        ch if !interp.is_empty() => out.push(ch),
                        _ => {}
                    }
                    i += 1;
                }
                i += 1;
                out.push('"');
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// An edit that leaves the tail of the block it replaced shows up here first: the stray `};`
/// closes something that was never opened.
#[test]
fn brackets_balance() {
    for (name, html) in scripted_pages() {
        let js = code_only(script(&html));
        let mut stack: Vec<(char, usize)> = Vec::new();
        let mut line = 1usize;
        for c in js.chars() {
            if c == '\n' {
                line += 1;
            }
            match c {
                '(' | '[' | '{' => stack.push((c, line)),
                ')' | ']' | '}' => {
                    let want = match c {
                        ')' => '(',
                        ']' => '[',
                        _ => '{',
                    };
                    match stack.pop() {
                        Some((open, _)) if open == want => {}
                        Some((open, at)) => {
                            panic!("{name} line {line}: '{c}' closes '{open}' opened on line {at}")
                        }
                        None => {
                            panic!("{name} line {line}: '{c}' closes nothing — leftover from an edit?")
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(stack.is_empty(), "{name}: unclosed {:?}", stack);
    }
}

/// `const show = …` next to the page's own `function show(status, kit)` is a SyntaxError, and a
/// SyntaxError anywhere takes the entire script with it.
#[test]
fn no_top_level_name_is_declared_twice() {
    let js = code_only(script(&page()));
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut dupes = Vec::new();
    for (n, raw) in js.lines().enumerate() {
        // Top level only: an indented `const` is inside something and may shadow freely.
        if raw.starts_with(' ') || raw.starts_with('\t') {
            continue;
        }
        let Some(rest) = ["const ", "let ", "function ", "var ", "class "]
            .iter()
            .find_map(|k| raw.strip_prefix(*k))
        else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        if name.is_empty() {
            continue;
        }
        if let Some(first) = seen.insert(name.clone(), n + 1) {
            dupes.push(format!("`{name}` on lines {first} and {}", n + 1));
        }
    }
    assert!(dupes.is_empty(), "declared twice at the top level: {dupes:?}");
}

/// `$('#gone').onclick = …` throws on a null, and it throws while the script is still setting
/// itself up, so nothing after it ever runs.
#[test]
fn every_selector_points_at_something() {
    let html = page();
    let js = script(&html);
    let mut missing = Vec::new();
    for (i, _) in js.match_indices("$('#") {
        let rest = &js[i + 4..];
        let Some(end) = rest.find('\'') else { continue };
        let id = &rest[..end];
        if !html.contains(&format!("id=\"{id}\"")) {
            missing.push(id.to_string());
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "handlers bound to ids that do not exist: {missing:?}");
}

/// Ids the markup declares more than once: `querySelector` silently takes the first, so the
/// second one is dead markup that looks alive.
#[test]
fn no_id_is_declared_twice() {
    for (name, html) in scripted_pages() {
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (i, _) in html.match_indices("id=\"") {
            let rest = &html[i + 4..];
            let Some(end) = rest.find('"') else { continue };
            *seen.entry(rest[..end].to_string()).or_default() += 1;
        }
        let dupes: Vec<_> =
            seen.iter().filter(|(_, n)| **n > 1).map(|(k, n)| format!("{k} × {n}")).collect();
        assert!(dupes.is_empty(), "{name}: duplicate ids: {dupes:?}");
    }
}

/// The reviewer's form binds by `$("id")` rather than the bay's `$('#id')`, and the failure is
/// the same one: `getElementById` returns null, the assignment throws while the script is still
/// setting itself up, and every handler after it is never bound. On this page that means a
/// physician opens the link, types for an hour and finds that Send does nothing.
#[test]
fn every_handle_on_the_review_form_points_at_something() {
    let html = review_page();
    let js = script(&html);
    let mut missing = Vec::new();
    for (i, _) in js.match_indices("$(\"") {
        let rest = &js[i + 3..];
        let Some(end) = rest.find('"') else { continue };
        let id = &rest[..end];
        if !html.contains(&format!("id=\"{id}\"")) {
            missing.push(id.to_string());
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "review.html binds ids that do not exist: {missing:?}");
}

/// The stamp the server replaces has to still be there for it to be replaced.
///
/// It is load-bearing twice over: it carries which build a reviewer's answers were written about,
/// and its *absence* after substitution is how the page knows it was served rather than mailed —
/// so a copy with no placeholder posts nowhere and a copy that never got stamped posts into the
/// void. `main.rs` holds the other half of this constant.
#[test]
fn the_build_placeholder_survives() {
    assert!(
        review_page().contains("__VITALS_BUILD__"),
        "review.html lost the stamp the server substitutes"
    );
}

/// The token the server injects has to still be there for it to be injected into.
/// **Where the control that ends an attempt is, and where it is not.**
///
/// This ends somebody's exam. The top bar holds `← episodes` and `restart` — two plain pills a
/// thumb's width apart, neither of which costs anything — and a third one beside them would be
/// the one irreversible act in the bay wearing the same coat as the two harmless ones, in the
/// one row of the page a hand passes over on the way to somewhere else.
///
/// So it lives at the foot of the working column instead — under history, orders, kit, chart and
/// disposition, which is the order the work actually happens in — and it arms before it fires.
/// Both of those are decisions rather than accidents, and a test is the only thing that keeps a
/// decision from being tidied away by the next person moving buttons around.
#[test]
fn the_control_that_ends_an_attempt_is_not_in_the_row_with_restart() {
    let html = page();
    let bar = {
        let a = html.find("<div class=\"bar\">").expect("no player bar");
        let b = html[a..].find("</div>").expect("unterminated bar") + a;
        &html[a..b]
    };
    assert!(bar.contains("id=\"back\"") && bar.contains("id=\"start\""),
            "the bar stopped holding the two buttons this test is measuring against");
    assert!(!bar.contains("id=\"endrun\""),
            "the control that ends the attempt has moved into the row with restart");
    // It is in the rail, after the disposition — the last thing in the column, not the first.
    let rail = html.find("<aside class=\"rail\">").expect("no rail");
    let end = html.find("id=\"endrun\"").expect("the finish control is gone from the page");
    let dispo = html.find("class=\"panel dispo\"").expect("no disposition panel");
    assert!(rail < dispo && dispo < end, "the finish control moved out from under the disposition");

    // And it arms rather than fires: the handler adds a class on the first press and only calls
    // the endpoint on the second.
    let h = html.find("$('#endrun').onclick").expect("nothing is bound to the finish control");
    let body = &html[h..h + 400];
    assert!(body.contains("armed"), "the finish control fires on one press");
    assert!(html.contains("/api/finish"), "the page never asks the server to finish anything");
}

#[test]
fn the_token_placeholder_survives() {
    assert!(page().contains("__VITALS_TOKEN__"), "the server replaces this at serve time");
}

/// Nothing may load over plain http, or the browser marks the whole page insecure.
///
/// The SVG namespace in the favicon's data: URI is exempt by name: `xmlns` is an identifier the
/// browser compares and never fetches, so it cannot trip mixed-content — the check stays strict
/// for everything that actually loads.
#[test]
fn no_insecure_subresource() {
    let html = page().replace("xmlns='http://www.w3.org/2000/svg'", "");
    assert!(!html.contains("http://"), "an http:// reference makes an https page 'Not Secure'");
}

/// The waveform must not be drawn through the lane's label.
///
/// It was: the trace was centred in the whole lane at 0.40 amplitude, so its peak reached 10% of
/// the lane height while "SINUS" and "25 mm/s" sit at 6px — an R wave straight through the text.
///
/// This is a shape check, not a geometry one: the real proof is measuring the topmost lit pixel
/// in the canvas, which is done by hand against a running monitor. What it guards is the specific
/// regression — going back to centring on the full lane height.
#[test]
fn the_monitor_reserves_a_band_for_its_labels() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/device/monitor.html");
    let html = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    assert!(
        html.contains("this.top"),
        "the trace no longer reserves a label band — labels will be drawn through"
    );
    assert!(
        !html.contains("this.h/2 -"),
        "the trace is centred on the whole lane again, which puts its peak inside the label"
    );
}

fn monitor() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/device/monitor.html");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The alarm limits are a function of age, and the numbers printed beside them are the same ones.
///
/// One hardcoded adult set — `hr:[50,120] … rr:[8,24]` — was applied to four paediatric cases.
/// A three-year-old with croup sits at 118 and 28, both normal for three, and the screen alarmed
/// on both from the first second of the station and printed "50–120 / 8–24" beside the numbers to
/// justify itself. This is a shape check on a file nothing type-checks: the arithmetic is
/// asserted below, in Rust, against the same table.
#[test]
fn the_monitor_bands_its_alarm_limits_by_age() {
    let html = monitor();
    assert!(html.contains("AGE_BANDS"), "the monitor has no age table");
    assert!(
        !html.contains("const LIM = { hr:[50,120]"),
        "the single adult limit set is back — every child on the season alarms on arrival"
    );
    // The limits printed beside each number are written from the table, not typed into markup:
    // a screen that alarms by one rule and explains itself by another is worse than either.
    for stale in ["<span class=\"lim\">50–120</span>", "<span class=\"lim\">8–24</span>",
                  "<span class=\"lim\">≥ 94</span>", "<span class=\"lim\">90–160</span>"] {
        assert!(!html.contains(stale), "a limit is still hardcoded in the markup: {stale}");
    }
    assert!(html.contains("$('#bed').textContent = BED"), "the bed label is hardcoded again");
    assert!(!html.contains("BED 3 · ER</span>"), "\"BED 3 · ER\" is printed over every case again");
}

/// The bands themselves, checked as arithmetic against the vitals the season's paediatric cases
/// actually open on. Reading a table out of JavaScript is fragile, so the table is restated here
/// and the *test* is that a well child does not alarm and a sick one does.
#[test]
fn a_normal_child_does_not_alarm_and_a_sick_one_does() {
    /// One row of the reference table: how old you have to be *under* to be in it, and the
    /// heart-rate, respiratory-rate and systolic ranges that count as normal there.
    struct Band {
        label: &'static str,
        under: f64,
        hr: (f64, f64),
        rr: (f64, f64),
        sbp: (f64, f64),
    }
    // Must match AGE_BANDS in static/device/monitor.html.
    let bands = [
        Band { label: "INFANT", under: 1.0, hr: (100.0, 160.0), rr: (30.0, 60.0), sbp: (70.0, 100.0) },
        Band { label: "1–2 YR", under: 3.0, hr: (90.0, 150.0), rr: (24.0, 40.0), sbp: (80.0, 110.0) },
        Band { label: "3–5 YR", under: 6.0, hr: (80.0, 140.0), rr: (22.0, 34.0), sbp: (85.0, 110.0) },
        Band { label: "6–11 YR", under: 12.0, hr: (70.0, 120.0), rr: (18.0, 30.0), sbp: (90.0, 120.0) },
        Band { label: "12–17 YR", under: 18.0, hr: (60.0, 100.0), rr: (12.0, 20.0), sbp: (100.0, 130.0) },
        Band { label: "ADULT", under: f64::INFINITY, hr: (50.0, 120.0), rr: (8.0, 24.0), sbp: (90.0, 160.0) },
    ];
    let html = monitor();
    for b in &bands {
        assert!(html.contains(b.label), "the monitor lost the {} band", b.label);
        let want = format!("hr:[{},{}]", b.hr.0 as i64, b.hr.1 as i64);
        assert!(
            html.replace(' ', "").contains(&want.replace(' ', "")),
            "{}: the heart-rate band moved away from {:?}",
            b.label,
            b.hr
        );
    }
    let band_for = |age: f64| bands.iter().find(|b| age < b.under).expect("adult catches everything");
    let out = |v: f64, (lo, hi): (f64, f64)| v > 0.0 && (v < lo || v > hi);

    // OSCE B3 — croup at three. demo/stations/osce-b3.sce.json opens her here, and every one of
    // these numbers is normal for a three-year-old. Not one may raise an alarm.
    let b = band_for(3.0);
    assert_eq!(b.label, "3–5 YR");
    assert!(!out(118.0, b.hr), "a three-year-old at 118 is a normal three-year-old");
    assert!(!out(28.0, b.rr), "a three-year-old breathing 28 is a normal three-year-old");
    assert!(!out(98.0, b.sbp), "a three-year-old at 98 systolic is a normal three-year-old");

    // OSCE C — croup at six, likewise well from the doorway.
    let c = band_for(6.0);
    assert_eq!(c.label, "6–11 YR");
    for (v, r) in [(105.0, c.hr), (22.0, c.rr), (100.0, c.sbp)] {
        assert!(!out(v, r), "OSCE C opens with a false alarm at {v}");
    }

    // OSCE D3 — anaphylaxis at six, twenty kilos, 138/82. The alarms here are the finding, and
    // banding by age must not silence them.
    assert!(out(138.0, c.hr), "a six-year-old at 138 is tachycardic and the screen must say so");
    assert!(out(82.0, c.sbp), "a six-year-old at 82 systolic is shocked and the screen must say so");

    // EP3 — epiglottitis at five. His heart rate of 148 is outside the band for five and still
    // alarms; his respiratory rate of 34 sits exactly on its ceiling and does not, which is the
    // banding working rather than failing — 34 at five is the top of normal, and the tachycardia
    // and the 93% saturation are what the screen is shouting about.
    let e = band_for(5.0);
    assert!(out(148.0, e.hr), "EP3's child must still alarm on his heart rate");
    assert!(!out(34.0, e.rr), "34 at five is the ceiling of normal, not over it");
    assert!(out(93.0, (94.0, 100.0)), "saturation does not band by age and must still alarm");

    // And the adult cases are exactly where they were. EP1 at 128/28 alarmed before and alarms now.
    let a = band_for(19.0);
    assert_eq!(a.label, "ADULT");
    assert!(out(128.0, a.hr) && out(28.0, a.rr), "EP1's alarms changed");
    assert!(!out(96.0, a.hr) && !out(20.0, a.rr), "EP2 gained an alarm it never had");
}

/// The pronoun table declines to guess, and nothing on the shelf makes it.
///
/// `pro()` used to end `? PRO_M : PRO_F` — anything that failed to match "· M" was a woman. That
/// is right about ten of the season's seventeen patients and silently wrong about the other seven,
/// and wrong in the worst available way, because a *failed match* renders identically to a
/// correct female match. A card added without a sex marker, or a `who` string edited into another
/// shape, would print "she goes home" over a man on the one frame that carries the hash, the mark
/// sheet and the anchor, and look completely normal doing it.
///
/// Two assertions, and the second is the one that matters: the fallback exists so a miss is
/// visible, and no shipped case may ever reach it.
#[test]
fn every_case_on_the_shelf_states_a_sex() {
    let html = page();
    assert!(html.contains("PRO_N="), "the neutral fallback is gone — a miss reads as a woman again");
    assert!(
        !html.contains("?PRO_M:PRO_F") && !html.contains("? PRO_M : PRO_F"),
        "pro() guesses female again on anything it cannot match"
    );

    // Every `who:` line in SEASON, checked against the marker pro() actually tests for.
    let season = html
        .split_once("const SEASON=[")
        .expect("no season table")
        .1
        .split_once("\n];")
        .expect("unterminated season table")
        .0;
    let whos: Vec<&str> = season
        .lines()
        .filter_map(|l| l.trim().strip_prefix("who:'"))
        .filter_map(|l| l.split_once('\''))
        .map(|(w, _)| w)
        .collect();
    assert_eq!(whos.len(), 17, "the season is not seventeen cases any more: {whos:?}");
    for w in &whos {
        assert!(
            w.contains("· M ") || w.contains("· F "),
            "{w:?} states no sex, so the bay would call this patient \"the patient\" all run"
        );
    }
    // And the split is what the copy claims it is — seven men, ten women.
    let men = whos.iter().filter(|w| w.contains("· M ")).count();
    assert_eq!(men, 7, "the season's seven men moved: {whos:?}");
}

/// The landing sells the season, not one patient of it.
///
/// Every line of copy on the front door was written for EP1 and said "her" — "Talk to her", "she
/// lives or she doesn't", "decides whether she lives" — while seven of the seventeen patients
/// behind that door are men. The paragraphs that are *about* Ing keep her: the frame is hers, the
/// alt text describes her, the card under the button is her card. What had to change is the copy
/// that describes the product.
#[test]
fn the_landing_sells_a_season_and_not_one_woman() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/landing.html");
    let html = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));

    for gendered in [
        "Talk to her", "Treat her", "She lives or she doesn't",
        "decides whether she lives", "give the drug you think she",
    ] {
        assert!(!html.contains(gendered), "the season-wide copy is EP1's again: {gendered:?}");
    }
    // EP1's own card is still EP1's — this is not a search-and-replace across the page.
    assert!(html.contains("Ing &middot; F 19"), "EP1's card lost her name");
    assert!(html.contains("her throat is closing"), "EP1's line was neutralised — it is about her");
    assert!(html.contains("alt=\"Ing, nineteen,"), "the photograph's alt text stopped describing her");

    // And the two things that must not move while copy is edited around them.
    assert!(html.contains("class=\"cf\" id=\"cf\""), "the coverflow lost its root");
    assert!(html.contains("class=\"patient\""), "the hero patient card is gone");
}

/// The bay sends the age, or the monitor cannot band anything. Read off the same `who` string the
/// bed label prints, so the age on the screen and the age in the limits cannot disagree.
#[test]
fn the_bay_tells_the_monitor_how_old_the_patient_is() {
    let html = page();
    assert!(html.contains("const ageOf="), "nothing reads an age out of the season table");
    assert!(html.contains("'&age='+a"), "the age never reaches the device");
    assert!(html.contains("'&bed='+encodeURIComponent(bedOf(e))"), "the bed label never reaches it");
}

// ── the donate page's fuel gauge ────────────────────────────────────────────
//
// Four numbers sit beside the donation address: the month's patient turns, the relay's balance
// and the runs it still buys, the treasury, and the count already anchored. Every one of them is
// fetched, never written — which is precisely why they are worth guarding mechanically. A
// hardcoded figure on this page is not a cosmetic bug; it is the page lying about the one thing
// it exists to prove.

fn donate() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/donate.html");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The renderer, without the vendored QR library it shares a `<script>` with.
fn fuel_js(html: &str) -> &str {
    let a = html.find("/* ── the fuel gauge renderer").expect("the fuel gauge renderer is gone");
    let b = html[a..].find("</script>").expect("unterminated <script>") + a;
    &html[a..b]
}

/// The same failure that takes the bay's page down takes this one's: a SyntaxError anywhere kills
/// the whole script, and then the gauge sits at "reading…" forever with no error in sight.
#[test]
fn the_fuel_renderer_balances_its_brackets() {
    let html = donate();
    let js = code_only(fuel_js(&html));
    let mut stack: Vec<char> = Vec::new();
    for c in js.chars() {
        match c {
            '(' | '[' | '{' => stack.push(c),
            ')' | ']' | '}' => {
                let want = match c {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                assert_eq!(stack.pop(), Some(want), "'{c}' closes the wrong thing");
            }
            _ => {}
        }
    }
    assert!(stack.is_empty(), "unclosed {stack:?}");
}

/// Every id the renderer writes into has to exist, or the first `.innerHTML` throws and the rest
/// of the gauge — including the failure branch that would have explained itself — never runs.
#[test]
fn every_id_the_gauge_writes_into_exists_in_the_markup() {
    let html = donate();
    let js = fuel_js(&html);
    let mut missing = Vec::new();
    for (i, _) in js.match_indices("$('") {
        let rest = &js[i + 3..];
        let Some(end) = rest.find('\'') else { continue };
        let id = &rest[..end];
        if !html.contains(&format!("id=\"{id}\"")) {
            missing.push(id.to_string());
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "the gauge writes into ids that do not exist: {missing:?}");
}

/// Not one figure on this page may be typed in.
///
/// The whole argument of the section is "read live, not written by hand", and a literal that
/// happens to be true on the day it was written is the exact way that argument dies quietly.
#[test]
fn no_figure_on_the_donate_page_is_hardcoded() {
    let html = donate();
    let js = fuel_js(&html);
    // The balances the server was returning while this was built, and the meter's ceiling. None
    // of them may appear as text: they arrive from /api/fuel or they do not appear at all.
    for literal in ["5.713747680", "190,458", "5,713,747,680", "20,000", "19,991", "0.000000000"] {
        assert!(!html.contains(literal), "{literal} is written into the page instead of read");
    }
    // The divisor is the server's constant, printed from the payload — never a second copy that
    // can drift away from the one the arithmetic actually used.
    assert!(js.contains("num(c.lamports_per_run)"), "the page stopped printing the server's divisor");
    assert!(js.contains("num(c.fee_lamports_per_tx)"), "the per-transaction fee is not shown");
    assert!(js.contains("c.tx_per_run"), "the transactions-per-run assumption is not shown");
    assert!(
        js.contains("num(r.lamports)"),
        "the exact lamport balance is not shown — SOL alone cannot be checked against an explorer"
    );
}

/// A reading that did not arrive must say so. All four, every time.
///
/// The failure this pins is the tempting one: leave the last good number on screen when the RPC
/// blinks. That turns a stale figure into a current claim, which is worse than an empty box.
#[test]
fn every_reading_has_a_branch_that_admits_it_failed() {
    let html = donate();
    let js = fuel_js(&html);
    for id in ["fTurns", "fRelay", "fVault", "fAnchored"] {
        assert!(js.contains(&format!("lost('{id}'")), "{id} has no failure branch");
    }
    assert!(js.contains(".catch(down)"), "an unreachable server leaves the old numbers on screen");
    assert!(js.contains("could not read this just now"), "a failure has to say so in words");
    // A failure keeps the way to check it by hand: the explorer, and the meter behind the turns.
    assert!(js.contains("audit the address yourself"), "a failed treasury read loses its explorer link");
}

/// Devnet SOL is minted free from a faucet. A balance shown without that sentence reads as an
/// asset, and this page is asking for money.
#[test]
fn the_page_says_out_loud_that_devnet_sol_is_not_money() {
    let html = donate();
    assert!(html.contains("minted free from a public faucet"), "the faucet caveat is gone");
    assert!(
        html.contains("model inference and hosting"),
        "the page stopped naming what donations actually pay for"
    );
    assert!(html.contains("fuel gauge"), "the SOL figure lost the framing that keeps it honest");
}

/// The things that were already on this page before the gauge arrived, and that a donation
/// depends on. A gauge that displaces the address is a gauge that costs money.
#[test]
fn the_gauge_did_not_displace_the_donation_itself() {
    let html = donate();
    assert!(html.contains("id=\"addrText\""), "the treasury address is gone");
    assert!(html.contains("9FJRwWnTNQXB9ff5SSmQKytCdVYqTQQPUz1b4zX9mt8y"), "the address changed");
    assert!(html.contains("data-ga=\"copy-address\""), "the copy button is gone");
    assert!(html.contains("id=\"qr\""), "the Solana Pay QR is gone");
    assert!(html.contains("audit the treasury yourself"), "the explorer link is gone");
    assert!(html.contains("solana:9FJRwWnTNQXB9ff5SSmQKytCdVYqTQQPUz1b4zX9mt8y"), "the QR payload changed");
}

/// Every item the review form asks fits the clamp the store keeps it under.
///
/// `review::Answer::asked` no longer holds a question — it holds the whole item as it was shown,
/// the four lines the review documents put in front of every ruling, because a reviewer has to be
/// able to answer from a phone with nothing open beside them. The clamp cuts silently, so an item
/// that outgrew it would arrive stored without the half of itself that says what was being asked,
/// and the answer would be unreadable a month later with nobody able to say why.
///
/// The measurement is deliberately an over-estimate: it sums *every* string in the item, including
/// the group heading, the group note and the option labels, none of which travel inside `asked`.
/// Failing this test means an item is close enough to the clamp to look at, not that it is over.
#[test]
fn every_item_the_review_form_asks_fits_the_clamp_the_store_keeps() {
    let html = review_page();
    let js = script(&html);
    let start = js.find("var QS = {").expect("the question table");
    let table = &js[start..];
    let items: Vec<&str> = table.split("\n      { ").skip(1).collect();
    assert!(items.len() > 30, "only found {} items — the split stopped working", items.len());

    let mut worst = (0usize, "");
    for item in &items {
        // Item text up to the next item; strings are plain double-quoted, because the table uses
        // typographic quotes (“ ”) inside its prose for exactly this reason.
        let mut total = 0usize;
        let mut rest = *item;
        while let Some(a) = rest.find('"') {
            let after = &rest[a + 1..];
            let Some(b) = after.find('"') else { break };
            total += after[..b].chars().count();
            rest = &after[b + 1..];
        }
        let id = item.split("id:\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("?");
        if total > worst.0 {
            worst = (total, id);
        }
    }
    assert!(
        worst.0 < 4000,
        "item `{}` sums to {} characters, at or past review::ASKED_MAX (4000) — it would be cut \
         on the way to disk, and the answer would arrive without its question",
        worst.1,
        worst.0
    );
    println!("longest item: {} at {} characters", worst.1, worst.0);
}

/// The form must offer a way to say "what you are doing now is correct".
///
/// Both review documents say it in as many words, and before the options existed that answer had
/// nowhere to go: it arrived as an empty box, was dropped as unanswered, and read exactly like an
/// item the reviewer never reached. A form that loses it costs a second round of asking.
#[test]
fn every_ruling_offers_a_way_to_agree_with_what_we_already_do() {
    let html = review_page();
    let js = script(&html);
    let table = &js[js.find("var QS = {").expect("the question table")..];
    let items: Vec<&str> = table.split("\n      { ").skip(1).collect();
    let mut optionless = Vec::new();
    for item in &items {
        let id = item.split("id:\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("?");
        if !item.contains("opts:[") {
            optionless.push(id.to_string());
        }
    }
    // One item has no options on purpose: the open question at the end of the student's document,
    // which asks whether the patients sound like people. There is no branch to pick there, and
    // offering one would be the form telling her what shape her answer should take.
    assert_eq!(
        optionless,
        vec!["people"],
        "these items give the reviewer no way to answer without writing prose: {optionless:?}"
    );
}

// ── the footers reach the policy ────────────────────────────────────────────
//
// A privacy policy nobody can find is the same defect as not having one. The two public pages a
// stranger actually lands on are the landing and the donate page, and both carry a footer — so
// the link lives there, and this is what stops a footer edit quietly dropping it.

/// Both public pages link to both documents, by the path the server actually serves.
#[test]
fn the_landing_and_donate_footers_link_to_the_policy_and_the_terms() {
    for name in ["landing.html", "donate.html"] {
        let html = static_page(name);
        for href in ["href=\"/privacy\"", "href=\"/terms\""] {
            assert!(html.contains(href), "{name} does not link {href}");
        }
    }
}

/// And the two documents link back to each other and to the front door, so a reader who arrives
/// at one is never stranded there.
#[test]
fn the_policy_and_the_terms_link_to_each_other() {
    let privacy = static_page("privacy.html");
    let terms = static_page("terms.html");
    assert!(privacy.contains("href=\"/terms\""), "the policy does not reach the terms");
    assert!(terms.contains("href=\"/privacy\""), "the terms do not reach the policy");
    for (name, html) in [("privacy.html", &privacy), ("terms.html", &terms)] {
        assert!(html.contains("href=\"/\""), "{name} has no way back to the front door");
        assert!(html.contains("paripol@megawiz.co"), "{name} names no way to reach a person");
    }
}

/// Thai text on this site uses Arabic digits, everywhere, including in a policy. The Thai
/// summary on the privacy page is the only Thai outside the game and the review form, and a Thai
/// numeral would be the one place it slipped in unnoticed.
#[test]
fn the_thai_summary_uses_arabic_digits() {
    let html = static_page("privacy.html");
    for (i, c) in html.chars().enumerate() {
        assert!(
            !('\u{0E50}'..='\u{0E59}').contains(&c),
            "a Thai numeral {c:?} at char {i} in privacy.html",
        );
    }
}

// ── what a page promises about data ─────────────────────────────────────────
//
// Not style checks. Each pins a sentence a person reads before doing something they cannot take
// back — signing a review with their name, or writing a run to a public chain — and each replaced
// a claim the code did not honour.

/// The review form never again promises that no address is kept.
///
/// It said “ไม่เก็บ IP ไม่ติดตามการใช้งาน” — we keep no IP, we do not track usage — and that was
/// wrong three times over: `/api/review` is rate-limited per address (`meter.rs`, keyed
/// `review:<ip>`), Cloud Run keeps its own request logs whatever this code does, and the page
/// fetches its fonts from Google before a reviewer types a character. Two clinicians read that
/// sentence and decided to sign their clinical opinion with their names. What replaced it is still
/// reassuring, because the true part is the reassuring part — nothing is stored beside the answers
/// — and it sends the reader to the policy for the rest.
#[test]
fn the_review_form_does_not_promise_that_no_address_is_kept() {
    let html = review_page();
    // Comments stripped first: the note in the file quotes the sentence it replaced, and a check
    // that cannot tell the quotation from the claim would forbid recording why the claim went.
    let mut visible = String::new();
    let mut rest = html.as_str();
    while let Some(a) = rest.find("<!--") {
        visible.push_str(&rest[..a]);
        rest = match rest[a..].find("-->") {
            Some(b) => &rest[a + b + 3..],
            None => "",
        };
    }
    visible.push_str(rest);
    assert!(
        !visible.contains("ไม่เก็บ IP"),
        "review.html claims again that it keeps no IP — `/api/review` is metered per address"
    );
    assert!(
        visible.contains("href=\"/privacy\""),
        "review.html says something about data and gives the reader nowhere to check it"
    );
}

/// The one irreversible control on the bay carries the fact beside it, and the link lands.
///
/// Anchoring writes a permanent, public, undeletable record. The policy says so at length, which
/// helps only a player who read the policy first — so the fact sits next to the button, tied to it
/// by `aria-describedby` so a screen reader reads it as part of the control, and the fragment it
/// points at has to exist in the page it points at.
#[test]
fn the_anchor_control_states_what_anchoring_does_and_the_link_resolves() {
    let html = page();
    let at = html.find("id=\"anchor\"").expect("index.html has no anchor button");
    let open = html[..at].rfind('<').expect("no tag around the anchor button");
    let close = at + html[at..].find('>').expect("unterminated anchor button tag");
    let tag = &html[open..close];
    assert!(
        tag.contains("aria-describedby=\"anchornote\""),
        "the anchor button is not tied to the line that says what anchoring costs: {tag}"
    );
    assert!(
        html.contains("id=\"anchornote\""),
        "index.html points at a note beside the anchor button that does not exist"
    );
    assert!(
        html.contains("href=\"/privacy#anchoring\""),
        "the note beside the anchor button does not reach the policy's anchoring section"
    );
    assert!(
        static_page("privacy.html").contains("id=\"anchoring\""),
        "the policy has no #anchoring for that link to land on"
    );
}

// ── the privacy policy against the code it describes ────────────────────────
//
// The page opens by claiming every sentence on it was written by reading the code, and §2 gives
// an enumerated list of what is kept in the visitor's browser. An enumerated list is a promise
// that nothing else is there, and it went one key short — `vitals.sets` shipped and the paragraph
// did not follow it. Nobody noticed because nothing was watching.

/// Every `vitals.` key any served page reads or writes must be named in the policy.
#[test]
fn the_policy_names_every_key_the_pages_keep_in_a_browser() {
    let policy = static_page("privacy.html");
    let mut found: Vec<String> = Vec::new();
    for name in ["index.html", "landing.html", "donate.html", "present.html", "review.html",
                 "terms.html", "privacy.html"] {
        let src = static_page(name);
        // `localStorage.getItem('vitals.x')` and its set/remove siblings, quoted either way.
        for (at, _) in src.match_indices("localStorage.") {
            let rest = &src[at..];
            let Some(open) = rest.find(['\'', '"']) else { continue };
            let q = rest.as_bytes()[open] as char;
            let Some(len) = rest[open + 1..].find(q) else { continue };
            let key = &rest[open + 1..open + 1 + len];
            if key.starts_with("vitals") && !found.iter().any(|k| k == key) {
                found.push(key.to_string());
            }
        }
    }
    assert!(found.len() >= 10, "the scanner found almost nothing — it has stopped working: {found:?}");
    for key in &found {
        assert!(
            policy.contains(key.as_str()),
            "{key} is written to a visitor's browser and §2 of the policy does not name it"
        );
    }
}

/// And the other direction, for the two the policy names by their hyphenated spelling: a key the
/// policy describes but no page uses is a policy describing a product that no longer exists.
#[test]
fn the_policy_names_no_key_the_pages_stopped_using() {
    let mut all = String::new();
    for name in ["index.html", "landing.html", "donate.html", "present.html", "review.html"] {
        all.push_str(&static_page(name));
    }
    for key in ["vitals-analytics-consent", "vitals-review-draft"] {
        assert!(all.contains(key), "the policy names {key} and no page uses it any more");
    }
}

/// §11 used to say *every* page loads Google's fonts. The three bedside screens never did — they
/// set type in what the machine has — and a policy that overstates what leaves the browser is
/// still a policy that is wrong about what leaves the browser.
#[test]
fn the_device_screens_reach_nothing_but_this_server() {
    for name in ["monitor.html", "vent.html", "pump.html"] {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/device").join(name);
        let src = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        for host in ["fonts.googleapis.com", "fonts.gstatic.com", "googletagmanager.com"] {
            assert!(
                !src.contains(host),
                "device/{name} now reaches {host}; §11 of the policy says it reaches nothing"
            );
        }
    }
}
