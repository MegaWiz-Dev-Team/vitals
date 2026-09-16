//! The interventions a compiled scenario defines: treatments from the plan's roles, the
//! diagnosis, the investigations, the examination, and the questions — with the patient's words
//! kept beside the questions as the ward's voice, never inside the engine's beats.

use crate::archetype::{Archetype, Kind};
use crate::embla::Case;
use crate::plan::{Mapped, Present};
use crate::text::{keywords, slug};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// One line of the patient's voice, keyed by the `ask_` intervention that draws it out.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceLine {
    pub finding: String,
    pub present: bool,
    /// `volunteered` | `on_ask` | `on_direct_ask` — the case's own reveal rule.
    pub reveal: String,
    /// The patient's words, verbatim from the case.
    pub words: String,
}

#[derive(Debug, Clone)]
pub struct Ask {
    pub id: String,
    pub present: bool,
    pub reveal: String,
}

#[derive(Debug, Clone)]
pub struct Built {
    pub interventions: Vec<Value>,
    pub voice: BTreeMap<String, VoiceLine>,
    pub asks: Vec<Ask>,
    /// `exam_` ids with the display and system they came from.
    pub exams: Vec<(String, String, String)>,
    /// `ix_` ids in the order of `expected_workup`, then the rest of the investigations.
    pub ix: Vec<(String, String)>,
    pub dx_id: String,
}

/// Caps and floors for one-off nudges, so an order can never push a vital past what a body
/// does or past the threshold that ends the case.
fn bounded_delta(var: &str, delta: f64) -> Value {
    let (cap, floor) = match var {
        "sbp" => (125.0, 30.0),
        "dbp" => (85.0, 15.0),
        "spo2" => (99.0, 40.0),
        "hr" => (180.0, 30.0),
        "rr" => (45.0, 4.0),
        "gcs" => (15.0, 3.0),
        _ => (100.0, 0.0),
    };
    if delta >= 0.0 {
        json!({ "delta": { var: delta }, "cap": cap })
    } else {
        json!({ "delta": { var: delta }, "floor": floor })
    }
}

fn tx(p: &Present, a: Archetype, mapped: &Mapped) -> Value {
    let role = &p.role;
    let id = p.tx_id();
    // The algorithm's own tools ask which state they are in; everything else is one effect list.
    let effects: Vec<Value> = match crate::acls::effects(role.id, a, mapped).or_else(|| a.branch_effects(role.id)) {
        Some(e) => e,
        None => {
            let mut effects: Vec<Value> = Vec::new();
            match role.kind {
                Kind::Harmful => {
                    for nd in role.nudges {
                        effects.push(bounded_delta(nd.var, nd.delta));
                    }
                    effects.push(json!({ "beat": role.beat }));
                }
                _ => {
                    effects.push(json!({ "flag": format!("{}_given", role.id) }));
                    for nd in role.nudges {
                        effects.push(bounded_delta(nd.var, nd.delta));
                    }
                    effects.push(json!({ "beat": role.beat }));
                }
            }
            effects
        }
    };
    let mut matcher = json!({ "any_kw": role.kw.iter().map(|k| crate::text::matcher_kw(k)).collect::<Vec<_>>() });
    if !role.not_kw.is_empty() {
        matcher["not_kw"] = json!(role.not_kw.iter().map(|k| crate::text::matcher_kw(k)).collect::<Vec<_>>());
    }
    let mut v = json!({
        "id": id,
        "label": role.label,
        "match": matcher,
        "effects": effects,
    });
    if let Some(h) = role.harm {
        v["harm"] = json!(h);
    }
    if let Some((eq, setting)) = role.equipment {
        v["equipment"] = json!(eq);
        if setting > 0.0 {
            v["equipment_setting"] = json!(setting);
        }
    }
    v
}

fn unique(id: String, taken: &mut BTreeSet<String>) -> String {
    if !taken.contains(&id) {
        taken.insert(id.clone());
        return id;
    }
    let mut k = 2;
    loop {
        let c = format!("{id}_{k}");
        if !taken.contains(&c) {
            taken.insert(c.clone());
            return c;
        }
        k += 1;
    }
}

/// The part of a display name an id is made from: before any parenthesis or dash, at most five
/// words — `CT abdomen only if stable and the diagnosis is unclear (must not delay laparotomy)`
/// becomes `ct_abdomen_only_if_stable`.
pub fn short_display(display: &str) -> String {
    let head = display.split([ '(', '—', ':' ]).next().unwrap_or(display);
    head.split_whitespace().take(5).collect::<Vec<_>>().join(" ")
}

fn id_for(prefix: &str, display: &str, n: usize, taken: &mut BTreeSet<String>) -> String {
    let s = slug(&short_display(display));
    let base = if s.is_empty() { format!("{prefix}_{n}") } else { format!("{prefix}_{s}") };
    unique(base, taken)
}

/// Harmful orders whose keywords are a *more specific* form of a therapy's — the IV push of the
/// drug that is right IM. Listed before everything else so the specific phrase wins the match.
const EARLY_HARMS: &[&str] = &["adrenaline_iv_push"];

/// Build the whole list, treatments first — the matcher takes the first intervention whose
/// keywords hit, and a drug name must beat a display word.
pub fn build(case: &Case, mapped: &Mapped, a: Archetype) -> Built {
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<Value> = Vec::new();

    for p in mapped.present.iter().filter(|p| p.role.kind == Kind::Harmful && EARLY_HARMS.contains(&p.role.id)) {
        taken.insert(p.tx_id());
        out.push(tx(p, a, mapped));
    }
    // treatments: critical, gate, rescue, supportive, harmful — the order the golden path follows
    let order = [Kind::Critical, Kind::Gate, Kind::Rescue, Kind::Supportive, Kind::Harmful];
    for kind in order {
        for p in mapped.present.iter().filter(|p| p.role.kind == kind && !(kind == Kind::Harmful && EARLY_HARMS.contains(&p.role.id))) {
            taken.insert(p.tx_id());
            out.push(tx(p, a, mapped));
        }
    }

    // the diagnosis
    let d = &case.hidden.correct_diagnosis;
    let short = d.aliases.iter().find(|a| a.is_ascii() && a.len() >= 4).cloned().unwrap_or_else(|| d.display.clone());
    let dx_id = id_for("dx", &short, 0, &mut taken);
    let mut dx_kw: Vec<String> = Vec::new();
    for a in d.aliases.iter().chain(std::iter::once(&d.display)) {
        let a = a.trim().to_lowercase();
        if a.chars().count() >= 4 && !dx_kw.contains(&a) {
            dx_kw.push(a);
        }
    }
    out.push(json!({
        "id": dx_id,
        "label": "Name the diagnosis",
        "match": { "any_kw": dx_kw },
        "effects": [ { "flag": "dx_named" }, { "beat": "the working diagnosis is written on the chart" } ],
    }));

    // investigations: the expected workup first (it is what the rubric pays for), results
    // attached where the case has them, then any remaining investigation with a result
    let mut ix: Vec<(String, String)> = Vec::new();
    let mut ix_seen: BTreeSet<String> = BTreeSet::new();
    let result_for = |display: &str| -> Option<String> {
        let want = slug(display);
        case.investigations.iter().find(|i| slug(&i.order.display) == want || (!want.is_empty() && slug(&i.order.display).starts_with(&want))).map(|i| {
            let r = &i.result;
            let mut s = crate::embla::ExamFinding { value: r.value.clone(), ..Default::default() }.value_text();
            if let Some(u) = &r.unit {
                if !u.is_empty() {
                    s.push_str(&format!(" ({u})"));
                }
            }
            if let Some(rep) = &r.report {
                if !rep.is_empty() {
                    s.push_str(&format!(" — {rep}"));
                }
            }
            s
        })
    };
    let workup: Vec<String> = case.hidden.expected_workup.iter().map(|w| w.display.clone()).filter(|w| !w.trim().is_empty()).collect();
    let extra: Vec<String> = case.investigations.iter().map(|i| i.order.display.clone()).filter(|w| !w.trim().is_empty()).collect();
    for (n, display) in workup.iter().chain(extra.iter()).enumerate() {
        let key = slug(display);
        let key = if key.is_empty() { display.clone() } else { key };
        if !ix_seen.insert(key) {
            continue;
        }
        let id = id_for("ix", display, n, &mut taken);
        let beat = match result_for(display) {
            Some(r) => format!("{display}: {r}"),
            None => format!("{display}: sent — result to follow"),
        };
        out.push(json!({
            "id": id,
            "label": display,
            "match": { "any_kw": keywords(display) },
            "effects": [ { "beat": beat } ],
        }));
        ix.push((id, display.clone()));
    }

    // examination: every non-vital finding, the value as the beat
    let mut exams: Vec<(String, String, String)> = Vec::new();
    for (n, f) in case.exam_findings.iter().filter(|f| !f.is_vital()).enumerate() {
        let display = if f.finding.display.trim().is_empty() { f.system.clone() } else { f.finding.display.clone() };
        let id = id_for("exam", &display, n, &mut taken);
        let mut kw = keywords(&display);
        for k in keywords(&f.system) {
            if !kw.contains(&k) {
                kw.push(k);
            }
        }
        let value = f.value_text();
        let beat = if value.trim().is_empty() { format!("{display}: examined") } else { value };
        out.push(json!({
            "id": id,
            "label": format!("Examine: {display}"),
            "match": { "any_kw": kw },
            "effects": [ { "beat": beat } ],
        }));
        exams.push((id, display, f.system.clone()));
    }

    // history: one ask per symptom line, the words kept in the voice
    let mut voice: BTreeMap<String, VoiceLine> = BTreeMap::new();
    let mut asks: Vec<Ask> = Vec::new();
    for (n, line) in case.symptom_script.iter().enumerate() {
        let display = line.finding.display.trim().to_string();
        if display.is_empty() {
            continue;
        }
        let id = id_for("ask", &display, n, &mut taken);
        let reveal = line.reveal.clone().unwrap_or_else(|| "on_ask".into());
        out.push(json!({
            "id": id,
            "label": format!("Ask: {display}"),
            "match": { "any_kw": keywords(&display) },
            "effects": [ { "beat": format!("history: {}", display.to_lowercase()) } ],
        }));
        voice.insert(id.clone(), VoiceLine {
            finding: display,
            present: line.present,
            reveal: reveal.clone(),
            words: line.patient_words.clone().unwrap_or_default(),
        });
        asks.push(Ask { id, present: line.present, reveal });
    }

    // A single word is a keyword only where it is distinctive — named by exactly one of the
    // displays the case defines. `abdominal` on both an X-ray and an examination would hand
    // whichever is listed first to a learner who meant the other; the phrase still matches
    // either, and the id always does.
    let mut word_count: BTreeMap<String, usize> = BTreeMap::new();
    for iv in &out {
        let id = iv["id"].as_str().unwrap_or_default();
        if id.starts_with("tx_") {
            continue;
        }
        if let Some(a) = iv["match"]["any_kw"].as_array() {
            for k in a.iter().filter_map(|k| k.as_str()) {
                if !k.contains(' ') {
                    *word_count.entry(k.to_string()).or_default() += 1;
                }
            }
        }
    }
    // keywords already claimed by an earlier intervention can never fire on a later one: drop
    // them there, and give every intervention its own id as a keyword of last resort
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    for iv in &mut out {
        let id = iv["id"].as_str().unwrap_or_default().to_string();
        let is_tx = id.starts_with("tx_");
        let kws: Vec<String> = iv["match"]["any_kw"]
            .as_array()
            .map(|a| a.iter().filter_map(|k| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let mut kept: Vec<String> = Vec::new();
        for k in kws {
            if claimed.iter().any(|c| c == &k || k.contains(c.as_str())) {
                continue;
            }
            if !is_tx && !k.contains(' ') && word_count.get(&k).copied().unwrap_or(0) > 1 {
                continue;
            }
            kept.push(k);
        }
        for k in &kept {
            claimed.insert(k.clone());
        }
        kept.push(id.clone());
        iv["match"]["any_kw"] = json!(kept);
    }

    Built { interventions: out, voice, asks, exams, ix, dx_id }
}
