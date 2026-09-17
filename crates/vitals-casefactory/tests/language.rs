//! World packs are compiled only from cases in English: a Thai story rendered under a persona
//! from elsewhere is a wrong sheet, and there is no translation step yet.

use vitals_casefactory::text::depersonalise;
use vitals_casefactory::{compile, Source};

const SYNTHETIC: &str = include_str!("fixtures/synthetic-septic-shock.json");

#[test]
fn a_thai_case_is_refused_by_language_before_anything_else_is_read() {
    let mut v: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    v["meta"]["language"] = serde_json::json!("th");
    let s = v.to_string();
    let err = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_err();
    assert_eq!(err.reason, "language: th — no translation step yet");
    // and a case that says nothing about its language is not assumed to be English
    let mut v: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    v["meta"].as_object_mut().unwrap().remove("language");
    let s = v.to_string();
    let err = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_err();
    assert!(err.reason.starts_with("language: unknown"), "{}", err.reason);
    // English, in any spelling of the tag, compiles
    for tag in ["en", "EN", "en-GB"] {
        let mut v: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
        v["meta"]["language"] = serde_json::json!(tag);
        let s = v.to_string();
        assert!(compile(&s, Source::of("embla-cases", "test", &s)).is_ok(), "{tag}");
    }
}

#[test]
fn placeholders_never_fire_inside_a_non_english_string() {
    // the sentence the ward showed: Thai prose must not gain an English fill
    let thai = "หญิงวัยกลางคนหายใจมีเสียงหวีด she said";
    assert_eq!(depersonalise(thai, Some(52), Some("female")), thai);
    let thai_age = "ผู้ป่วยชายอายุ 26 ปี มาด้วยไข้";
    assert_eq!(depersonalise(thai_age, Some(26), Some("male")), thai_age);
    // English prose still gets them
    assert_eq!(depersonalise("a 26-year-old man", Some(26), Some("male")), "a {age}-year-old {sex_word}");
    // a mostly-English line with one Thai word is English
    assert_eq!(depersonalise("He asked for ยาแก้ปวด", Some(26), Some("male")), "{He_she} asked for ยาแก้ปวด");
}

#[test]
fn the_report_lists_the_cases_refused_by_language_in_their_own_section() {
    use vitals_casefactory::report::{render, Outcome};
    use vitals_casefactory::Refusal;
    let results = vec![
        ("ddx-thai-1".to_string(), Outcome::Refused(Refusal { case_id: "ddx-thai-1".into(), reason: "language: th — no translation step yet".into() })),
        ("ddx-thai-2".to_string(), Outcome::Refused(Refusal { case_id: "ddx-thai-2".into(), reason: "language: th — no translation step yet".into() })),
        ("ddx-stable".to_string(), Outcome::Refused(Refusal { case_id: "ddx-stable".into(), reason: "no archetype fits: 'Acne' names no deterioration this library models (stable presentation, OPD tier 1)".into() })),
    ];
    let r = render(&results, "lib", "worktree", None);
    assert!(r.contains("## Refused by language"), "{r}");
    assert!(r.contains("| th | 2 |"), "{r}");
    assert!(r.contains("ddx-thai-1") && r.contains("ddx-thai-2"));
    assert!(r.contains("refused 3"));
}
