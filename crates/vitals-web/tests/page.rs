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
#[test]
fn no_insecure_subresource() {
    let html = page();
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
