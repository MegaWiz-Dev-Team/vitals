//! Small text tools: ids out of prose, keywords out of display names, negation in a sentence.

use std::collections::BTreeSet;

/// An ASCII identifier from a display name: `Thick and thin blood film` → `thick_and_thin_blood_film`.
/// Non-ASCII (a Thai display) yields an empty slug and the caller falls back to a numbered id.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut last_us = true;
    for ch in s.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_us = false;
        } else if !last_us {
            out.push('_');
            last_us = true;
        }
        if out.len() >= 40 {
            break;
        }
    }
    out.trim_matches('_').to_string()
}

/// Words a learner would not type to mean a specific order — kept out of matcher keywords so
/// `chest` does not hand a chest X-ray to whoever says "listen to the chest".
const STOP: &[&str] = &[
    "the", "and", "with", "for", "from", "that", "this", "than", "then", "into", "over", "under", "after",
    "before", "since", "about", "without", "within", "test", "tests", "level", "levels", "count", "profile",
    "examination", "assessment", "general", "chest", "blood", "urine", "bedside", "point", "care", "serial",
    "repeat", "including", "only", "when", "where", "which", "while", "still", "very", "more", "most",
    "less", "left", "right", "both", "sides", "first", "second", "days", "day", "hours", "hour", "week",
    "weeks", "month", "months", "year", "years", "morning", "night", "today", "yesterday", "patient",
    "history", "symptoms", "signs", "status", "present", "absent", "known", "unknown", "recent", "normal",
    "abnormal", "positive", "negative", "other", "some", "none", "not", "no", "yes", "one", "two", "three",
    "four", "five", "six", "seven", "eight", "nine", "ten", "any", "all", "per", "via", "to", "of", "in",
    "on", "at", "by", "or", "if", "is", "as", "an", "a", "x", "ml", "mg", "kg", "min", "sec", "regional",
    "exclusion", "loinc", "value", "result", "immediately", "hourly", "daily", "appearance", "findings",
    "finding", "output", "sample", "bloods", "area", "areas",
];

/// Matcher keywords for a display name: the whole phrase, then each distinctive word.
///
/// Distinctive means four letters or more, not a stop word, not a number. The phrase comes first
/// so a learner who types the order as written matches it whole; the words let a shorter order
/// land. Everything is lower-case because the engine lower-cases both sides.
pub fn keywords(display: &str) -> Vec<String> {
    let phrase = display.trim().to_lowercase();
    let mut out: Vec<String> = Vec::new();
    if phrase.len() >= 4 {
        out.push(phrase.clone());
    }
    for w in phrase.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '₂').map(str::trim) {
        let w = w.trim_matches('-');
        if w.len() < 4 || STOP.contains(&w) || w.chars().all(|c| c.is_ascii_digit()) || out.iter().any(|o| o == w) {
            continue;
        }
        out.push(w.to_string());
    }
    out
}

/// Split a plan step or a red flag into the fragments a negation applies to.
pub fn fragments(s: &str) -> Vec<String> {
    s.split([';', '.', '—', ':', '\n'])
        .map(|f| f.trim().to_lowercase())
        .filter(|f| !f.is_empty())
        .collect()
}

const NEGATIONS: &[&str] = &[
    "do not", "don't", "never", "avoid", "must not", "should not", "not indicated", "not routinely", "contraindicated",
    "no ", "not be used", "stop all", "stop ", "withhold", "is not acceptable", "cannot", "not acceptable",
    "ห้าม", "หลีกเลี่ยง", "ไม่ควร", "งด",
];

/// Does this fragment forbid something?
pub fn negated(fragment: &str) -> bool {
    let f = fragment.to_lowercase();
    NEGATIONS.iter().any(|n| f.contains(n))
}

/// Seconds named by a time-critical phrase — `within 1 hour`, `within the first hour`,
/// `Hour-1`, `within 5 minutes`, `at once`, `immediately`. `None` when the sentence names no time.
pub fn time_named(s: &str) -> Option<f64> {
    let f = s.to_lowercase();
    let re_num = regex::Regex::new(r"within (?:the first )?(\d+)\s*(min|minute|minutes|h|hr|hour|hours)\b").ok()?;
    if let Some(c) = re_num.captures(&f) {
        let n: f64 = c[1].parse().ok()?;
        let unit = &c[2];
        return Some(if unit.starts_with('m') { n * 60.0 } else { n * 3600.0 });
    }
    if f.contains("within the first hour") || f.contains("hour-1") || f.contains("first hour") {
        return Some(3600.0);
    }
    if f.contains("at once") || f.contains("immediately") || f.contains("now,") || f.contains(" now") || f.contains("ทันที") {
        return Some(300.0);
    }
    None
}

/// Every string anywhere in a JSON value, with a path, for the scans that refuse a pack.
pub fn strings(v: &serde_json::Value, path: &str, out: &mut Vec<(String, String)>) {
    match v {
        serde_json::Value::String(s) => out.push((path.to_string(), s.clone())),
        serde_json::Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                strings(x, &format!("{path}[{i}]"), out);
            }
        }
        serde_json::Value::Object(o) => {
            for (k, x) in o {
                out.push((format!("{path}.{k}"), k.clone()));
                strings(x, &format!("{path}.{k}"), out);
            }
        }
        _ => {}
    }
}

/// Name tokens worth scanning for: each word of the patient's name of three letters or more that
/// is not a title. Case-insensitive on the caller's side.
pub fn name_tokens(name: &str) -> BTreeSet<String> {
    const TITLES: &[&str] = &["mr", "mrs", "ms", "miss", "dr", "นาย", "นาง", "นางสาว", "ด.ช.", "ด.ญ.", "เด็กชาย", "เด็กหญิง"];
    name.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| w.chars().count() >= 3 && !TITLES.contains(&w.as_str()))
        .collect()
}

/// Replace the patient's name in a piece of prose with a neutral placeholder — `the patient`
/// for a Latin-script name matched as whole words, `ผู้ป่วย` for a Thai one matched as a
/// substring (Thai has no word spaces). The ward assigns its own persona; the case's opening
/// must not carry a name the persona will contradict.
pub fn scrub(text: &str, name: &str) -> String {
    let mut out = text.to_string();
    for t in name_tokens(name) {
        if t.is_ascii() {
            let mut rebuilt = String::with_capacity(out.len());
            let low = out.to_lowercase();
            let mut i = 0;
            while i < out.len() {
                let rest = &low[i..];
                if rest.starts_with(t.as_str()) {
                    let before = low[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
                    let after = low[i + t.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
                    if before && after {
                        rebuilt.push_str("the patient");
                        i += t.len();
                        continue;
                    }
                }
                let ch = out[i..].chars().next().unwrap_or(' ');
                rebuilt.push(ch);
                i += ch.len_utf8();
            }
            out = rebuilt;
        } else {
            out = out.replace(t.as_str(), "ผู้ป่วย");
        }
    }
    out
}
