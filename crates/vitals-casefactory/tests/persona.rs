//! The ward assigns its own persona, so the pack's prose must not state the Embla patient's age
//! or sex as facts: they become placeholders the ward's renderer fills.

use vitals_casefactory::text::{depersonalise, prose_leaks};
use vitals_casefactory::{compile, Source};

const SYNTHETIC: &str = include_str!("fixtures/synthetic-septic-shock.json");

#[test]
fn age_and_sex_words_become_placeholders_with_their_capital_kept() {
    let s = depersonalise("Young man from Bangladesh, a 26-year-old garment-factory worker. He came with his roommate; we asked him.", Some(26), Some("male"));
    assert_eq!(s, "Young {sex_word} from Bangladesh, a {age}-year-old garment-factory worker. {He_she} came with {his_her} roommate; we asked {him_her}.");
    let s = depersonalise("She is 8 years old; her mother carried her in. Aged 8.", Some(8), Some("female"));
    assert_eq!(s, "{He_she} is {age} years old; {his_her} mother carried {him_her} in. Aged {age}.");
}

#[test]
fn the_other_sex_and_other_peoples_ages_stay_and_so_does_the_body() {
    // a male patient: "she" is somebody else; the son's age is the son's
    let s = depersonalise("He says she gave him the tablets; his 4-year-old son is well. Pregnant women in the compound.", Some(34), Some("male"));
    assert_eq!(s, "{He_she} says she gave {him_her} the tablets; {his_her} 4-year-old son is well. Pregnant women in the compound.");
    // a female patient: "pregnant" is the body, not the persona
    let s = depersonalise("A pregnant woman, 29-year-old, her third pregnancy.", Some(29), Some("female"));
    assert_eq!(s, "A pregnant {sex_word}, {age}-year-old, {his_her} third pregnancy.");
}

#[test]
fn thai_prose_is_left_as_written_and_the_scan_still_names_what_it_states() {
    // Ruling of 17 Sep: a placeholder is filled with an English word, so it never fires inside
    // non-English prose. The language gate refuses such cases upstream; if one ever got
    // through, the scan would still refuse the pack rather than ship a Thai sentence that names
    // the patient's sex — the belt to that brace.
    use vitals_casefactory::text::prose_leaks;
    let s = depersonalise("ผู้ป่วยชายอายุ 26 ปี มาด้วยไข้", Some(26), Some("male"));
    assert_eq!(s, "ผู้ป่วยชายอายุ 26 ปี มาด้วยไข้");
    let leaks = prose_leaks(&s, Some(26), Some("male"));
    assert!(leaks.iter().any(|l| l.contains("age")) && leaks.iter().any(|l| l.contains("sex word")), "{leaks:?}");
}

#[test]
fn the_scan_names_what_the_prose_still_states() {
    let leaks = prose_leaks("a 26-year-old man, aged 26, and his wife", Some(26), Some("male"));
    assert!(leaks.iter().any(|l| l.contains("26-year-old")), "{leaks:?}");
    assert!(leaks.iter().any(|l| l.contains("man")), "{leaks:?}");
    assert!(leaks.iter().any(|l| l.contains("his")), "{leaks:?}");
    assert!(prose_leaks("a {age}-year-old {sex_word}, {his_her} wife", Some(26), Some("male")).is_empty());
    // another person's age and the other sex are not leaks
    assert!(prose_leaks("his 4-year-old son; she is his wife", Some(34), Some("male")).iter().all(|l| l.contains("his")));
}

#[test]
fn a_compiled_pack_carries_placeholders_not_the_patients_facts() {
    let mut v: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    v["meta"]["title"] = serde_json::json!("Young woman with fever, now cold and confused");
    v["presentation"]["hpi"] = serde_json::json!("A 41-year-old woman; she stopped her antibiotics early. Her sister brought her.");
    v["symptom_script"][0]["patient_words"] = serde_json::json!("I have been shaking with fever for three days. My sister says she noticed it first.");
    v["exam_findings"][5]["value"] = serde_json::json!("Pale, sweaty woman; cold clammy peripheries, capillary refill 4 seconds; drowsy but rousable (GCS 14, E3 V5 M6)");
    let s = v.to_string();
    let pack = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_or_else(|e| panic!("{}", e.reason));
    assert_eq!(pack.title, "Young {sex_word} with fever, now cold and confused");
    assert_eq!(pack.presentation.hpi, "A {age}-year-old {sex_word}; {he_she} stopped {his_her} antibiotics early. {His_her} sister brought {him_her}.");
    // the patient's own "she" in a voice line: the speaker is the patient talking about her sister — the
    // same-sex rule cannot tell, and says so in the docs; the words stay verbatim except for the placeholders
    let text = pack.sce.to_string();
    assert!(text.contains("sweaty {sex_word}"), "exam beat: {text}");
    assert!(!text.contains("41-year-old"));
    assert_eq!(pack.patient.age, Some(41));
    assert_eq!(pack.patient.sex.as_deref(), Some("female"));
    assert!(pack.placeholders.age >= 1 && pack.placeholders.sex >= 5, "{:?}", pack.placeholders);
}
