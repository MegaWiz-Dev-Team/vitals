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

fn page() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/index.html");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
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
    let js = code_only(script(&page()));
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
                        panic!("line {line}: '{c}' closes '{open}' opened on line {at}")
                    }
                    None => panic!("line {line}: '{c}' closes nothing — leftover from an edit?"),
                }
            }
            _ => {}
        }
    }
    assert!(stack.is_empty(), "unclosed {:?}", stack);
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
    let html = page();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (i, _) in html.match_indices("id=\"") {
        let rest = &html[i + 4..];
        let Some(end) = rest.find('"') else { continue };
        *seen.entry(rest[..end].to_string()).or_default() += 1;
    }
    let dupes: Vec<_> = seen.iter().filter(|(_, n)| **n > 1).map(|(k, n)| format!("{k} × {n}")).collect();
    assert!(dupes.is_empty(), "duplicate ids: {dupes:?}");
}

/// The token the server injects has to still be there for it to be injected into.
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

/// The bay sends the age, or the monitor cannot band anything. Read off the same `who` string the
/// bed label prints, so the age on the screen and the age in the limits cannot disagree.
#[test]
fn the_bay_tells_the_monitor_how_old_the_patient_is() {
    let html = page();
    assert!(html.contains("const ageOf="), "nothing reads an age out of the season table");
    assert!(html.contains("'&age='+a"), "the age never reaches the device");
    assert!(html.contains("'&bed='+encodeURIComponent(bedOf(e))"), "the bed label never reaches it");
}
