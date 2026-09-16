//! Which physiology archetype a case compiles under — and the refusal when none fits.

mod common;

use vitals_casefactory::archetype::Archetype;
use vitals_casefactory::embla::parse_case;

const SYNTHETIC: &str = include_str!("fixtures/synthetic-septic-shock.json");

#[test]
fn the_synthetic_shock_case_is_septic_shock() {
    let case = parse_case(SYNTHETIC).unwrap();
    let v0 = case.vitals0().unwrap();
    let a = Archetype::detect(&case, &v0).expect("fits");
    assert_eq!(a, Archetype::SepticShock);
    assert_eq!(a.id(), "septic_shock");
}

#[test]
fn a_shock_word_in_a_red_flag_does_not_make_a_normotensive_patient_a_shock_case() {
    // The words fit; the vitals do not. The compiler must not force it.
    let mut case: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    case["exam_findings"][0]["value"] = serde_json::json!("128/82 mmHg");
    case["exam_findings"][1]["value"] = serde_json::json!("88/min");
    let case = parse_case(&case.to_string()).unwrap();
    let v0 = case.vitals0().unwrap();
    let err = Archetype::detect(&case, &v0).unwrap_err();
    assert!(err.contains("not forced"), "{err}");
}

#[test]
fn a_case_with_no_deterioration_words_is_refused_with_the_diagnosis_in_the_reason() {
    let mut case: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    case["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": "Acne vulgaris", "aliases": [] });
    case["hidden"]["red_flags"] = serde_json::json!([]);
    case["meta"]["search_tags"] = serde_json::json!([]);
    case["meta"]["title"] = serde_json::json!("Spots");
    case["meta"]["specialty"] = serde_json::json!("eir-dermatology");
    let case = parse_case(&case.to_string()).unwrap();
    let v0 = case.vitals0().unwrap();
    let err = Archetype::detect(&case, &v0).unwrap_err();
    assert!(err.contains("no archetype fits") && err.contains("Acne vulgaris"), "{err}");
}

#[test]
fn each_of_the_six_endemic_cases_has_its_archetype() {
    let Some(dir) = common::embla_dir() else { return common::skip("embla-cases not present"); };
    let want = [
        ("embla-severe-falciparum-malaria-cerebral-resident", Archetype::CnsDepressionHypoglycaemia),
        ("embla-lassa-fever-haemorrhagic-shock-resident", Archetype::HaemorrhagicShock),
        ("embla-acute-chagas-myocarditis-cardiogenic-shock-resident", Archetype::CardiogenicShock),
        ("embla-typhoid-ileal-perforation-septic-shock-intern", Archetype::SepticShock),
        ("embla-neurotoxic-krait-envenoming-respiratory-failure-intern", Archetype::NeuromuscularRespiratoryFailure),
        ("embla-dengue-shock-syndrome-child-intern", Archetype::PaediatricCompensatedShock),
    ];
    for (id, a) in want {
        let Some(json) = common::show(&dir, common::ENDEMIC_REF, id) else {
            return common::skip("endemic branch not present");
        };
        let case = parse_case(&json).unwrap();
        let v0 = case.vitals0().unwrap_or_else(|e| panic!("{id}: {e}"));
        let got = Archetype::detect(&case, &v0).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(got, a, "{id}");
    }
}

#[test]
fn a_stable_clinic_case_from_the_library_is_refused() {
    let Some(dir) = common::embla_dir() else { return common::skip("embla-cases not present"); };
    let Some(json) = common::library_case(&dir, "auth-acne-vulgaris-y4-1") else {
        return common::skip("acne case not in library");
    };
    let case = parse_case(&json).unwrap();
    match case.vitals0() {
        Ok(v0) => {
            let err = Archetype::detect(&case, &v0).unwrap_err();
            assert!(err.contains("no archetype fits"), "{err}");
        }
        Err(e) => assert!(e.contains("exam_findings"), "{e}"),
    }
}
