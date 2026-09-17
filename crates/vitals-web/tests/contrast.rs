//! **What the page asks a stranger to read, and whether they can read it.**
//!
//! UX review G2 and H1. Two things a stylesheet can be held to without a browser:
//!
//!   * **contrast.** Every colour the pages use for text, against the grounds they sit on, at the
//!     WCAG 2.1 ratio. 4.5:1 is the floor for body text. The secondary ink was 3.15:1 on the
//!     globe's own ground and 2.92:1 on the bay's cards — which is the legend, the footer, every
//!     mono label, and the "view" button on a patient whose stay has ended.
//!   * **a focus ring that exists.** A `:focus-visible` rule that sets `outline: none` and nothing
//!     else is a control a keyboard cannot find. `a.take` — the way into every bed — was exactly
//!     that: the hover rule and the focus rule shared a block, and the block turned the outline off
//!     so the hover would look clean.
//!
//! Computed rather than eyeballed, because a ratio is a number and a number can be wrong in a way
//! a screenshot is not. What a screenshot is still for: the things this cannot see — a colour over
//! a photograph, text on the monitor's black, the globe's own ramp.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// Every `<style>` block of a page, or the whole file when it is a stylesheet.
fn css_of(rel: &str) -> String {
    let src = read(rel);
    if !rel.ends_with(".html") {
        return src;
    }
    let mut out = String::new();
    let mut rest = src.as_str();
    while let Some(i) = rest.find("<style>") {
        rest = &rest[i + 7..];
        let Some(end) = rest.find("</style>") else { break };
        out.push_str(&rest[..end]);
        out.push('\n');
        rest = &rest[end..];
    }
    out
}

/// Every custom property the page really has, merged in the order the cascade would.
///
/// Three things this has to get right, each of which it got wrong first:
///   * the declarations start *after* the brace — the first one shares a `;`-segment with `:root{`
///     otherwise, and the page's own ground goes missing without a word;
///   * a stylesheet may hold several `:root` blocks (the bay has the leaderboard's palette, then
///     the ward's), and the later one wins the way it does in a browser;
///   * a `:root` inside `@media (prefers-color-scheme: dark)` is a different palette for a
///     different reader and is not merged into this one. Depth tells them apart.
fn tokens(css: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    // Byte indices throughout, because this file is full of box-drawing comments and a char index
    // into a Rust string is not a byte index — the first attempt panicked inside a '─'.
    let (mut depth, mut i) = (0usize, 0usize);
    while i < css.len() {
        if !css.is_char_boundary(i) {
            i += 1;
            continue;
        }
        match css[i..].chars().next().unwrap_or(' ') {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 && css[i..].starts_with(":root") => {
                let Some(open) = css[i..].find('{').map(|p| i + p + 1) else { break };
                let Some(close) = css[open..].find('}').map(|p| open + p) else { break };
                for decl in css[open..close].split(';') {
                    if let Some((k, v)) = decl.split_once(':') {
                        let k = k.trim();
                        if k.starts_with("--") {
                            out.insert(k.to_string(), v.trim().to_string());
                        }
                    }
                }
                i = close;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn rgb(hex: &str) -> (f64, f64, f64) {
    let h = hex.trim().trim_start_matches('#');
    let full: String = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect() } else { h.to_string() };
    let part = |i: usize| u8::from_str_radix(&full[i..i + 2], 16).unwrap_or(0) as f64;
    (part(0), part(2), part(4))
}

/// WCAG 2.1 relative luminance.
fn luminance(hex: &str) -> f64 {
    let f = |v: f64| {
        let v = v / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = rgb(hex);
    0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

fn ratio(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn the_contrast_of_every_text_colour_on_every_ground_it_sits_on() {
    // Each pair is one the pages really make: the token used for text, the token under it, and
    // where to look if it fails.
    let pages: &[(&str, &[(&str, &str, &str)])] = &[
        ("static/world/index.html", &[
            ("--ink", "--ground", "the headline and the body text"),
            ("--ink-2", "--ground", "the mission line, the intro, the panel's empty state"),
            ("--ink-3", "--ground", "the legend, the hint, the footer, the figures' labels"),
            ("--ink-3", "--surface", "the panel's own labels and the country card"),
            ("--ink-3", "--surface-2", "a portrait placeholder's initials"),
            ("--ink-2", "--surface", "a patient row's meta line"),
            ("--proven", "--ground", "the headline's second line"),
            ("--proven", "--surface", "the take button and the state pills"),
            ("--proven", "--proven-soft", "a pressed switch and the on-shift pill"),
        ]),
        ("static/bay.css", &[
            ("--ink", "--room", "the bedside card's name and the case sheet"),
            ("--ink-2", "--room", "the sheet's body text"),
            ("--ink-3", "--room", "the strip's clock, the chip labels, the disposition notes"),
            ("--ink-3", "--card", "the quiet exit, the tray's labels"),
            ("--ink-2", "--card", "the transcript"),
            ("--act", "--card", "every primary control's own text"),
            ("--alarm", "--card", "the hand-over button and the lease running out"),
        ]),
    ];

    let mut thin = Vec::new();
    for (page, pairs) in pages {
        let t = tokens(&css_of(page));
        for (fg, bg, what) in *pairs {
            let (Some(a), Some(b)) = (t.get(*fg), t.get(*bg)) else {
                panic!("{page}: {fg} or {bg} is not a token there any more");
            };
            let r = ratio(a, b);
            if r < 4.5 {
                thin.push(format!("{page}: {fg} {a} on {bg} {b} is {r:.2}:1 — {what}"));
            }
        }
    }
    assert!(thin.is_empty(),
            "text below 4.5:1, which is the floor for body text and every one of these is body \
             text:\n  {}", thin.join("\n  "));
}

#[test]
fn every_focus_rule_shows_something() {
    let mut blind = Vec::new();
    for page in ["static/world/index.html", "static/bay.css"] {
        let css = css_of(page);
        let mut rest = css.as_str();
        while let Some(i) = rest.find(":focus-visible") {
            // Back to the start of the selector, forward to the end of the block.
            let sel_start = rest[..i].rfind('}').map(|p| p + 1).unwrap_or(0);
            let Some(open) = rest[i..].find('{').map(|p| i + p) else { break };
            let Some(close) = rest[open..].find('}').map(|p| open + p) else { break };
            let sel = rest[sel_start..open].trim().replace('\n', " ");
            let body = &rest[open + 1..close];
            let shows = body.contains("box-shadow")
                || body.split(';').any(|d| {
                    let d = d.trim();
                    d.starts_with("outline") && !d.contains("none") && !d.starts_with("outline-offset")
                });
            if !shows {
                blind.push(format!("{page}: {sel} {{{}}}", body.trim().replace('\n', " ")));
            }
            rest = &rest[close..];
        }
    }
    assert!(blind.is_empty(),
            "a :focus-visible rule that shows nothing is a control a keyboard cannot find:\n  {}",
            blind.join("\n  "));
}
