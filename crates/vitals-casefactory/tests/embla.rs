//! An Embla `case.json` is read into the handful of facts the compiler needs — and the vitals,
//! which arrive as prose in a dozen spellings, come out as numbers the engine can start from.

use vitals_casefactory::embla::{parse_case, Vitals0};

const SYNTHETIC: &str = include_str!("fixtures/synthetic-septic-shock.json");

#[test]
fn a_case_parses_into_its_meta_and_its_starting_vitals() {
    let case = parse_case(SYNTHETIC).expect("the synthetic fixture parses");
    assert_eq!(case.meta.id, "synthetic-septic-shock-test");
    assert_eq!(case.meta.difficulty.as_deref(), Some("intern"));
    assert_eq!(case.meta.country.as_deref(), Some("ZZZ"));
    assert_eq!(case.meta.clinical_tier, Some(5));
    assert_eq!(case.patient.age, Some(41));

    let v = case.vitals0().expect("the fixture carries a full vital-sign set");
    assert_eq!(v.hr, 130.0);
    assert_eq!(v.sbp, 82.0);
    assert_eq!(v.dbp, 50.0);
    assert_eq!(v.spo2, 94.0);
    assert_eq!(v.rr, 28.0);
    assert!((v.temp - 38.9).abs() < 1e-9);
    // Written inside the general-appearance sentence, not under its own heading.
    assert_eq!(v.gcs, 14);
    assert!(v.assumed.is_empty(), "nothing had to be assumed: {:?}", v.assumed);
}

#[test]
fn vitals_written_as_one_combined_line_still_parse() {
    // The Thai library often puts the whole set on one line under one heading.
    let mut case: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    case["exam_findings"] = serde_json::json!([
        { "system": "Vitals", "finding": { "display": "Hypotension" },
          "value": "BP 80/40 mmHg, HR 120 bpm, RR 28 bpm, Temp 39.5 C, O2 sat 90% room air" }
    ]);
    let case = parse_case(&case.to_string()).unwrap();
    let v: Vitals0 = case.vitals0().unwrap();
    assert_eq!((v.sbp, v.dbp, v.hr, v.rr, v.spo2), (80.0, 40.0, 120.0, 28.0, 90.0));
    assert!((v.temp - 39.5).abs() < 1e-9);
    // No GCS anywhere: assumed 15, and the pack says so.
    assert_eq!(v.gcs, 15);
    assert_eq!(v.assumed, vec!["gcs"]);
}

#[test]
fn a_case_without_a_blood_pressure_cannot_start_a_monitor() {
    let mut case: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    case["exam_findings"] = serde_json::json!([
        { "system": "vitals", "finding": { "display": "Heart rate" }, "value": "130/min" }
    ]);
    let case = parse_case(&case.to_string()).unwrap();
    let err = case.vitals0().unwrap_err();
    assert!(err.contains("blood pressure"), "{err}");
}

#[test]
fn a_fahrenheit_temperature_is_read_in_celsius() {
    let mut case: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    case["exam_findings"][4]["value"] = serde_json::json!("102.0 °F");
    let case = parse_case(&case.to_string()).unwrap();
    let v = case.vitals0().unwrap();
    assert!((v.temp - 38.9).abs() < 0.05, "{}", v.temp);
}

#[test]
fn the_two_of_spo2_is_not_a_saturation() {
    let mut case: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    case["exam_findings"] = serde_json::json!([
        { "system": "vitals", "finding": { "display": "Hypotension" }, "value": "BP 80/50 mmHg" },
        { "system": "vitals", "finding": { "display": "Tachycardia" }, "value": "HR 120 bpm" },
        { "system": "vitals", "finding": { "display": "Hypoxia" }, "value": "SpO2 85% on room air" }
    ]);
    let case = parse_case(&case.to_string()).unwrap();
    let v = case.vitals0().unwrap();
    assert_eq!((v.sbp, v.hr, v.spo2), (80.0, 120.0, 85.0));
    assert_eq!(v.assumed, vec!["rr", "temp", "gcs"]);
}

#[test]
fn a_past_history_of_plain_strings_parses_like_one_of_coded_entries() {
    let mut v: serde_json::Value = serde_json::from_str(include_str!("fixtures/synthetic-septic-shock.json")).unwrap();
    v["presentation"]["pmh"] = serde_json::json!(["Hypertension", "Type 2 diabetes"]);
    let case = vitals_casefactory::embla::parse_case(&v.to_string()).unwrap();
    assert_eq!(case.presentation.pmh.iter().map(|p| p.display.as_str()).collect::<Vec<_>>(), vec!["Hypertension", "Type 2 diabetes"]);
    v["presentation"]["pmh"] = serde_json::json!([{ "display": "Hypertension", "icd10_who": "I10" }]);
    let case = vitals_casefactory::embla::parse_case(&v.to_string()).unwrap();
    assert_eq!(case.presentation.pmh[0].display, "Hypertension");
    // and the pack never carries it
    let s = v.to_string();
    let pack = vitals_casefactory::compile(&s, vitals_casefactory::Source::of("embla-cases", "test", &s)).unwrap();
    assert!(serde_json::to_value(&pack).unwrap()["presentation"].get("pmh").is_none());
}
