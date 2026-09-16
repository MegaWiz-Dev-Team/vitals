//! The prose fields of a pack — what the ward renders as words — and the two passes over them:
//! the rewrite that puts the persona placeholders in, and the scan that refuses what remains.
//!
//! Prose is: the title, the presentation, every `beat`, `label` and `harm` string in the
//! scenario and the rubric, and the voice. The plan steps and the timed sentences are quoted
//! verbatim for the reviewer and are not prose the ward shows a stranger.

use crate::text::Persona;
use crate::Placeholders;
use serde_json::Value;

const PROSE_KEYS: [&str; 6] = ["beat", "label", "harm", "title", "chief_complaint", "hpi"];

/// Apply `f` to every prose string under `v`, in place.
pub fn rewrite(v: &mut Value, f: &dyn Fn(&str) -> String) {
    match v {
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                if PROSE_KEYS.contains(&k.as_str()) {
                    if let Value::String(s) = x {
                        *s = f(s);
                    }
                } else {
                    rewrite(x, f);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| rewrite(x, f)),
        _ => {}
    }
}

fn prose_strings(v: &Value, path: &str, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                let p = format!("{path}.{k}");
                match x {
                    Value::String(s) if PROSE_KEYS.contains(&k.as_str()) || k == "words" || k == "finding" || k == "setting" => out.push((p, s.clone())),
                    _ => prose_strings(x, &p, out),
                }
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                prose_strings(x, &format!("{path}[{i}]"), out);
            }
        }
        _ => {}
    }
}

/// Every prose string in the pack that still states the patient's age or sex.
pub fn scan(pack: &Value, persona: &Persona) -> Vec<String> {
    let mut all = Vec::new();
    // the reviewer-facing quotations are not prose the ward renders
    let mut trimmed = pack.clone();
    if let Value::Object(o) = &mut trimmed {
        o.remove("management");
        o.remove("timed");
        o.remove("source");
        o.remove("compiler");
    }
    prose_strings(&trimmed, "$", &mut all);
    let mut errs = Vec::new();
    for (path, s) in all {
        for l in persona.leaks(&s) {
            errs.push(format!("{l} at {path}"));
        }
    }
    errs
}

/// Count the placeholders across the given values.
pub fn count(values: &[Value]) -> Placeholders {
    let mut all = Vec::new();
    for v in values {
        prose_strings(v, "$", &mut all);
    }
    let mut p = Placeholders::default();
    for (_, s) in all {
        p.age += s.matches("{age}").count();
        let low = s.to_lowercase();
        for ph in ["{sex_word}", "{he_she}", "{his_her}", "{him_her}", "{himself_herself}"] {
            p.sex += low.matches(ph).count();
        }
    }
    p
}
