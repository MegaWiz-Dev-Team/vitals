//! The Embla `case.json`, read leniently, and its vital signs read out of prose.
//!
//! Lenient on purpose. The library has 433 cases written over a year by several hands and two
//! generators; a field that is a string in one case is an object in the next, and a case that
//! fails to *parse* is a case the ward silently never gets. Everything optional is `Option` or
//! defaulted, and only the facts the compiler actually needs are typed. What is not needed is not
//! read — the presentation prose, the differential probabilities, the SNOMED codes.

use regex::Regex;
use serde::Deserialize;
use std::sync::OnceLock;

/// Something with a `display` name, which is how Embla writes every coded concept.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Named {
    #[serde(default)]
    pub display: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Meta {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub specialty: Option<String>,
    #[serde(default)]
    pub difficulty: Option<String>,
    #[serde(default)]
    pub care_setting: Option<String>,
    #[serde(default)]
    pub clinical_tier: Option<u8>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub search_tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Patient {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub age: Option<u32>,
    #[serde(default)]
    pub sex: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SymptomLine {
    #[serde(default)]
    pub finding: Named,
    #[serde(default)]
    pub present: bool,
    #[serde(default)]
    pub reveal: Option<String>,
    #[serde(default)]
    pub patient_words: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExamFinding {
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub finding: Named,
    /// Prose in the library, occasionally a number. Read as text either way.
    #[serde(default)]
    pub value: serde_json::Value,
}

impl ExamFinding {
    pub fn value_text(&self) -> String {
        value_text(&self.value)
    }
    /// Is this row a vital sign? Embla spells the system `vitals`, `Vitals`, `Vital signs`.
    pub fn is_vital(&self) -> bool {
        self.system.to_lowercase().contains("vital")
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct InvestigationResult {
    #[serde(default)]
    pub value: serde_json::Value,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub flag: Option<String>,
    #[serde(default)]
    pub report: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Investigation {
    #[serde(default)]
    pub order: Named,
    #[serde(default)]
    pub result: InvestigationResult,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Diagnosis {
    #[serde(default)]
    pub display: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Dimension {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub weight: f64,
    #[serde(default)]
    pub scoring: String,
    #[serde(default)]
    pub criteria: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Rubric {
    #[serde(default)]
    pub pass_mark: Option<f64>,
    #[serde(default)]
    pub dimensions: Vec<Dimension>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Hidden {
    #[serde(default)]
    pub correct_diagnosis: Diagnosis,
    #[serde(default)]
    pub expected_workup: Vec<Named>,
    #[serde(default)]
    pub red_flags: Vec<String>,
    #[serde(default)]
    pub management_plan: Vec<String>,
    #[serde(default)]
    pub rubric: Rubric,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Case {
    pub meta: Meta,
    #[serde(default)]
    pub patient: Patient,
    #[serde(default)]
    pub symptom_script: Vec<SymptomLine>,
    #[serde(default)]
    pub exam_findings: Vec<ExamFinding>,
    #[serde(default)]
    pub investigations: Vec<Investigation>,
    #[serde(default)]
    pub hidden: Hidden,
}

/// Read a case. The error is the serde message — a case that does not parse is reported, per
/// case, in the compile report, and never silently dropped.
pub fn parse_case(json: &str) -> Result<Case, String> {
    serde_json::from_str(json).map_err(|e| format!("case.json does not parse: {e}"))
}

fn value_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

// ── vital signs out of prose ────────────────────────────────────────────────────

/// The vital-sign vector a scenario starts from, plus the list of what had to be assumed.
#[derive(Debug, Clone, PartialEq)]
pub struct Vitals0 {
    pub hr: f64,
    pub sbp: f64,
    pub dbp: f64,
    pub spo2: f64,
    pub rr: f64,
    pub temp: f64,
    pub gcs: u8,
    /// Which fields were not in the case and were filled with a resting default. Carried into the
    /// pack so a reviewer sees an assumption rather than a measurement.
    pub assumed: Vec<String>,
}

fn re(s: &'static str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(s).expect("a literal regex compiles"))
}

static RE_BP: OnceLock<Regex> = OnceLock::new();
static RE_INT: OnceLock<Regex> = OnceLock::new();
static RE_DEC: OnceLock<Regex> = OnceLock::new();
static RE_GCS: OnceLock<Regex> = OnceLock::new();
static RE_TOK_BP: OnceLock<Regex> = OnceLock::new();
static RE_TOK_HR: OnceLock<Regex> = OnceLock::new();
static RE_TOK_RR: OnceLock<Regex> = OnceLock::new();
static RE_TOK_SPO2: OnceLock<Regex> = OnceLock::new();
static RE_TOK_TEMP: OnceLock<Regex> = OnceLock::new();

fn first_bp(s: &str) -> Option<(f64, f64)> {
    let r = re(r"(\d{2,3})\s*/\s*(\d{2,3})", &RE_BP);
    let c = r.captures(s)?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?))
}
/// The first whole number that stands on its own — not the `2` of `SpO2` or the `12` of `12-lead`.
fn first_int(s: &str) -> Option<f64> {
    let r = re(r"(?:^|[^A-Za-z0-9])(\d{1,3})(?:[^0-9.]|$)", &RE_INT);
    r.captures(s)?[1].parse().ok()
}
fn first_dec(s: &str) -> Option<f64> {
    let r = re(r"(?:^|[^A-Za-z0-9])(\d{2,3}(?:\.\d+)?)", &RE_DEC);
    r.captures(s)?[1].parse().ok()
}

/// Fahrenheit is read as Fahrenheit. A temperature above 45 is not a living human in Celsius.
fn celsius(t: f64, text: &str) -> f64 {
    if t > 45.0 || text.contains("°F") || text.contains(" F") {
        ((t - 32.0) * 5.0 / 9.0 * 10.0).round() / 10.0
    } else {
        t
    }
}

fn is(display: &str, any: &[&str]) -> bool {
    let d = display.to_lowercase();
    any.iter().any(|k| d.contains(k))
}

impl Case {
    /// The starting vitals, read off `exam_findings`.
    ///
    /// Two passes. First each vital under its own heading — the shape the generated cases use.
    /// Then, for anything still missing, the combined line the Thai library favours
    /// (`BP 80/40 mmHg, HR 120 bpm, RR 28 bpm, Temp 39.5 C, O2 sat 90%`), scanned by token across
    /// every vital-sign row. A blood pressure, a heart rate and a respiratory rate are required —
    /// a monitor cannot start without them. Saturation, temperature and GCS default to resting
    /// values when absent, and the default is written down in `assumed`.
    pub fn vitals0(&self) -> Result<Vitals0, String> {
        let rows: Vec<(String, String)> = self
            .exam_findings
            .iter()
            .filter(|f| f.is_vital())
            .map(|f| (f.finding.display.clone(), f.value_text()))
            .collect();

        let mut bp = None;
        let mut hr = None;
        let mut rr = None;
        let mut spo2 = None;
        let mut temp = None;

        for (d, v) in &rows {
            if bp.is_none() && is(d, &["blood pressure", "bp", "ความดัน", "hypotension", "hypertension", "hemodynamic"]) {
                bp = first_bp(v);
            }
            if hr.is_none() && is(d, &["heart rate", "pulse", "ชีพจร", "tachycardia", "bradycardia", "hr"]) && !is(d, &["blood pressure"]) {
                hr = first_int(v);
            }
            if rr.is_none() && is(d, &["respiratory rate", "breathing rate", "อัตราการหายใจ", "rr"]) && !is(d, &["heart rate"]) {
                rr = first_int(v);
            }
            if spo2.is_none() && is(d, &["saturation", "spo2", "spo₂", "o2 sat", "oxygen", "hypoxia"]) {
                spo2 = first_int(v);
            }
            if temp.is_none() && is(d, &["temperature", "temp", "อุณหภูมิ", "fever", "hyperthermia", "hypothermia", "bt"]) {
                temp = first_dec(v).map(|t| celsius(t, v));
            }
        }

        // Second pass: the combined line, or a value that names its own vitals — across every
        // examination row, because a heart rate sometimes lives under `cardiovascular`.
        let all: String = self.exam_findings.iter().map(|f| format!("{}: {}", f.finding.display, f.value_text())).collect::<Vec<_>>().join(" | ");
        if bp.is_none() {
            let r = re(r"(?i)\bBP\b\D{0,12}(\d{2,3})\s*/\s*(\d{2,3})", &RE_TOK_BP);
            if let Some(c) = r.captures(&all) {
                bp = Some((c[1].parse().unwrap_or(0.0), c[2].parse().unwrap_or(0.0)));
            }
        }
        if hr.is_none() {
            let r = re(r"(?i)\b(?:HR|PR|pulse)\b\D{0,12}(\d{2,3})", &RE_TOK_HR);
            hr = r.captures(&all).and_then(|c| c[1].parse().ok());
        }
        if rr.is_none() {
            let r = re(r"(?i)\bRR\b\D{0,12}(\d{1,3})", &RE_TOK_RR);
            rr = r.captures(&all).and_then(|c| c[1].parse().ok());
        }
        if spo2.is_none() {
            let r = re(r"(?i)(?:SpO2|SpO₂|O2 sat|sat)\D{0,12}(\d{2,3})\s*%", &RE_TOK_SPO2);
            spo2 = r.captures(&all).and_then(|c| c[1].parse().ok());
        }
        if temp.is_none() {
            let r = re(r"(?i)\b(?:Temp|T|BT)\b\D{0,6}(\d{2,3}(?:\.\d+)?)", &RE_TOK_TEMP);
            temp = r.captures(&all).and_then(|c| c[1].parse::<f64>().ok()).map(|t| celsius(t, &all));
        }

        let (sbp, dbp) = bp.ok_or_else(|| "no blood pressure in exam_findings — a monitor cannot start without one".to_string())?;
        let hr = hr.ok_or_else(|| "no heart rate in exam_findings".to_string())?;
        if sbp <= dbp || !(40.0..=300.0).contains(&sbp) {
            return Err(format!("blood pressure {sbp:.0}/{dbp:.0} is not a blood pressure"));
        }

        let mut assumed = Vec::new();
        // The respiratory rate is the vital the library most often leaves out. A resting 18 is
        // assumed and written down; the heart rate and the pressure are never assumed.
        let rr = rr.unwrap_or_else(|| { assumed.push("rr".into()); 18.0 });
        let spo2 = spo2.unwrap_or_else(|| { assumed.push("spo2".into()); 97.0 });
        let temp = temp.unwrap_or_else(|| { assumed.push("temp".into()); 37.0 });
        let gcs = self.gcs().unwrap_or_else(|| { assumed.push("gcs".into()); 15 });

        Ok(Vitals0 { hr, sbp, dbp, spo2, rr, temp, gcs, assumed })
    }

    /// The Glasgow Coma Scale, wherever the case wrote it: under its own heading, inside the
    /// general-appearance sentence (`GCS 14, E3 V5 M6`), or as `AVPU: V` (read as 13).
    pub fn gcs(&self) -> Option<u8> {
        let r = re(r"(?i)\b(?:GCS|Glasgow Coma Scale)\b\D{0,6}(\d{1,2})", &RE_GCS);
        for f in &self.exam_findings {
            let text = format!("{} {}", f.finding.display, f.value_text());
            if let Some(c) = r.captures(&text) {
                if let Ok(n) = c[1].parse::<u8>() {
                    if (3..=15).contains(&n) {
                        return Some(n);
                    }
                }
            }
            let t = text.to_uppercase();
            if t.contains("AVPU: V") || t.contains("AVPU:V") || t.contains("AVPU V") {
                return Some(13);
            }
            if t.contains("AVPU: P") {
                return Some(9);
            }
            if t.contains("AVPU: U") {
                return Some(3);
            }
        }
        None
    }

    /// The patient's sex as one lower-case word, if the case says.
    pub fn sex(&self) -> Option<String> {
        let s = value_text(self.patient.sex.as_ref()?).to_lowercase();
        match s.as_str() {
            "m" | "male" | "ชาย" => Some("male".into()),
            "f" | "female" | "หญิง" => Some("female".into()),
            "" => None,
            other => Some(other.to_string()),
        }
    }

    /// Everything the compiler reads when deciding which archetype fits, lower-cased, in one
    /// string: the diagnosis and its aliases, the red flags, the tags, the specialty, the title.
    pub fn haystack(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.hidden.correct_diagnosis.display);
        s.push(' ');
        for a in &self.hidden.correct_diagnosis.aliases {
            s.push_str(a);
            s.push(' ');
        }
        for r in &self.hidden.red_flags {
            s.push_str(r);
            s.push(' ');
        }
        for t in &self.meta.search_tags {
            s.push_str(t);
            s.push(' ');
        }
        s.push_str(self.meta.specialty.as_deref().unwrap_or(""));
        s.push(' ');
        s.push_str(&self.meta.title);
        s.to_lowercase()
    }
}
