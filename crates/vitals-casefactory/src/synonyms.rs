//! The names a doctor actually types for a diagnosis, keyed by the name the case gives it.
//!
//! A case's `correct_diagnosis.display` is written for a reader — "Urinary tract infection with
//! urosepsis", "Bronchospasm / acute asthma exacerbation" — and a doctor at a bedside writes
//! "urosepsis" or "asthma". Until 23 Sep 2026 the diagnosis intervention answered to the display
//! name and nothing else, and the first stranger to finish a shift on production typed the plain
//! name of the disease and was scored 0 of 10 for naming it. The names live here, in one file the
//! compiler owns (`data/diagnosis_synonyms.json`), and every name in it is subject to the same
//! rules as an author's alias: four characters or more (the engine matches by substring, so
//! "af" would fire inside "after"), checked against the case's own differential, typed every
//! way it gets typed.
//!
//! The key is exact — NFKC, lower-case, one space between words — on purpose: "pneumonia" must
//! never attach to "pneumocystis pneumonia" or "measles pneumonia", whose names contain the word
//! and whose diagnoses are something else.
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Where the table lives, as the refusal names it.
pub const FILE: &str = "crates/vitals-casefactory/data/diagnosis_synonyms.json";

const TABLE: &str = include_str!("../data/diagnosis_synonyms.json");

fn table() -> &'static BTreeMap<String, Vec<String>> {
    static T: OnceLock<BTreeMap<String, Vec<String>>> = OnceLock::new();
    T.get_or_init(|| {
        let raw: BTreeMap<String, Vec<String>> =
            serde_json::from_str(TABLE).expect("data/diagnosis_synonyms.json is a JSON object: name → [names]");
        raw.into_iter()
            .map(|(k, v)| (key(&k), v.iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect()))
            .collect()
    })
}

/// The lookup key: NFKC, lower-case, trimmed, inner whitespace collapsed to one space.
pub fn key(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfkc().collect::<String>().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The table's names for exactly this name — the display or one alias — or none.
pub fn names_for(name: &str) -> &'static [String] {
    table().get(&key(name)).map(Vec::as_slice).unwrap_or(&[])
}

/// Every key the table holds, for the tests that read the table against the catalogue.
pub fn keys() -> impl Iterator<Item = &'static str> {
    table().keys().map(String::as_str)
}
