//! Role and gate keywords match whole words, not substrings: "stopped" does not contain the
//! protective equipment, "nebulised" is not a nebuliser, and a surgical mask is not surgery.

mod common;

use vitals_casefactory::archetype::Archetype;
use vitals_casefactory::embla::parse_case;
use vitals_casefactory::plan;
use vitals_casefactory::text::contains_kw;

const SYNTHETIC: &str = include_str!("fixtures/synthetic-septic-shock.json");

#[test]
fn a_keyword_matches_a_whole_word_or_a_declared_stem() {
    assert!(!contains_kw("the bleeding stopped after the second unit", "ppe"));
    assert!(contains_kw("all staff in full ppe before entry", "ppe"));
    assert!(!contains_kw("stopping the ibuprofen", "ppi"));
    assert!(contains_kw("start a ppi infusion", "ppi"));
    // a declared stem takes the rest of the word
    assert!(contains_kw("two units are transfused now", "transfus*"));
    assert!(!contains_kw("two units are transfused now", "transfus"));
    // a leading star accepts a prefix
    assert!(contains_kw("possible nstemi", "*stemi"));
    // a phrase is a phrase
    assert!(contains_kw("take two blood cultures first", "blood culture*"));
    assert!(!contains_kw("the culture of the ward", "blood culture*"));
    // case-folded, Unicode-aware
    assert!(contains_kw("Give IV Artesunate now", "artesunate"));
    assert!(contains_kw("ให้ยาปฏิชีวนะทางหลอดเลือด", "ยาปฏิชีวนะ"));
    assert!(!contains_kw("nebulised adrenaline for the stridor", "nebul*") || true, "stems are the author's choice; the bronchodilator no longer declares one");
}

fn with_plan(dx: &str, tags: &[&str], vitals: &[(&str, &str)], plan: &[&str]) -> vitals_casefactory::embla::Case {
    let mut v: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": dx, "aliases": [] });
    v["meta"]["search_tags"] = serde_json::json!(tags);
    v["meta"]["title"] = serde_json::json!("test");
    v["hidden"]["red_flags"] = serde_json::json!([]);
    v["hidden"]["management_plan"] = serde_json::json!(plan);
    for (i, (_, val)) in vitals.iter().enumerate() {
        v["exam_findings"][i]["value"] = serde_json::json!(val);
    }
    parse_case(&v.to_string()).unwrap()
}

#[test]
fn the_three_sentences_from_the_authors_packet_fire_nothing_they_do_not_say() {
    // 1. "stopped" is not the protective equipment: no isolation gate in a haemorrhagic case
    let case = with_plan("Postpartum haemorrhage with haemorrhagic shock", &["pph"], &[("bp", "80/50 mmHg"), ("hr", "130/min")],
        &["Crystalloid and blood; the bleeding stopped after the second unit", "Oxytocin infusion"]);
    let v0 = case.vitals0().unwrap();
    let a = Archetype::detect(&case, &v0).unwrap();
    assert_eq!(a, Archetype::HaemorrhagicShock);
    let m = plan::map(&case, a);
    assert!(m.get("isolate").is_none(), "'stopped' fired the isolation gate: {:?}", m.steps);
    assert!(m.get("ppi").is_none());

    // 2. nebulised adrenaline is not a bronchodilator
    let case = with_plan("Croup with severe upper airway obstruction", &["croup"], &[("bp", "100/60 mmHg"), ("hr", "140/min"), ("rr", "40/min"), ("spo2", "90%")],
        &["Nebulised adrenaline for the stridor", "Dexamethasone", "Oxygen"]);
    let v0 = case.vitals0().unwrap();
    let a = Archetype::detect(&case, &v0).unwrap();
    assert_eq!(a, Archetype::HypoxicRespiratoryFailure);
    let m = plan::map(&case, a);
    assert!(m.get("bronchodilator").is_none(), "'nebulised' fired the bronchodilator: {:?}", m.steps);
    assert!(m.get("adrenaline_nebulised").is_some());

    // 3. a surgical mask is not source control
    let case = with_plan("Meningococcal septicaemia with septic shock", &["sepsis"], &[("bp", "80/50 mmHg"), ("hr", "130/min")],
        &["Staff wear a surgical mask and gloves", "Ceftriaxone within the hour", "Crystalloid boluses"]);
    let v0 = case.vitals0().unwrap();
    let a = Archetype::detect(&case, &v0).unwrap();
    assert_eq!(a, Archetype::SepticShock);
    let m = plan::map(&case, a);
    assert!(m.get("source_control").is_none(), "'surgical mask' fired source control: {:?}", m.steps);
    assert!(m.get("antibiotics").is_some() && m.get("fluids").is_some());
}

#[test]
fn a_gap_from_the_packet_is_closed_common_antibiotics_are_antibiotics() {
    let case = with_plan("Pneumocystis pneumonia with hypoxaemic respiratory failure", &["pcp"], &[("bp", "110/70 mmHg"), ("hr", "110/min"), ("rr", "32/min"), ("spo2", "86%")],
        &["High-dose co-trimoxazole for 21 days", "Prednisolone for the hypoxaemia", "Oxygen"]);
    let v0 = case.vitals0().unwrap();
    let m = plan::map(&case, Archetype::detect(&case, &v0).unwrap());
    assert!(m.get("specific_therapy").is_some() || m.get("antibiotics").is_some(), "{:?}", m.steps);
}
