//! The patient answers in the language the learner chose — and the case does not change with it.
//!
//! The bay's whole claim is that a run is comparable to every other run of the same case. A
//! language layer is the easiest way to break that quietly: translate the case notes and the
//! patient now has a different allergy, a different timeline, or one symptom fewer, and nobody
//! finds out until a rubric marks a candidate down for missing a fact their patient never said.
//!
//! So the brief is built one way and one way only — the authored English notes, verbatim, plus an
//! instruction about which language to *speak* them in. These tests read the brief that would go
//! on the wire without a model anywhere near them, which is the point: the thing that must be
//! right is checkable on a laptop with no gateway, in CI, on every commit.

use vitals_web::lang;
use vitals_web::patient::brief;

fn ep1() -> serde_json::Value {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demo/ep1-en.json");
    serde_json::from_str(&std::fs::read_to_string(&p).expect("demo/ep1-en.json — EP1's text layer"))
        .expect("EP1's text layer is JSON")
}

/// The default encounter is untouched. Not "equivalent" — the same string it was before the
/// language layer landed, with no clause about language bolted onto it.
#[test]
fn the_english_brief_gained_nothing() {
    let s = brief(&ep1(), "Deteriorating", 91.0, lang::default_language());
    assert!(s.contains("in first person, in English,"), "she stopped being told to speak English");
    assert!(!s.contains("LANGUAGE."), "the default encounter grew a translation instruction");
}

/// Thai is the English brief **plus** an instruction — which is the design stated as an
/// assertion. Every authored fact reaches the model in the words the case author wrote, and the
/// only thing the language adds is a demand about how to say them.
#[test]
fn a_translated_brief_is_the_english_one_with_an_instruction_after_it() {
    let p = ep1();
    let en = brief(&p, "Deteriorating", 91.0, lang::default_language());
    let th = brief(&p, "Deteriorating", 91.0, lang::language(Some("th")));

    let head = en.split("in first person, in English,").next().expect("a head");
    assert!(th.starts_with(head), "the two briefs diverge before the language is even named");
    assert!(th.contains("in first person, in Thai,"), "she was not told which language to speak");
    assert!(th.contains("LANGUAGE."), "the language instruction is missing");

    // The notes are the case and they arrive in English, undiluted. If this ever fails, somebody
    // has started translating the record — which mints a different case.
    for fact in ["allergic to shrimp", "ten minutes ago", "left it at home"] {
        assert!(en.contains(fact), "EP1's notes changed: {fact}");
        assert!(th.contains(fact), "{fact} was dropped or pre-translated out of the Thai brief");
    }

    // And the instruction says the one thing it has to say.
    for demand in ["do not change them", "Do not add a detail", "and only in Thai"] {
        assert!(th.contains(demand), "the brief does not forbid: {demand}");
    }
}

/// Every language in the table produces a brief that names itself, so adding Bahasa Indonesia is
/// a row in `LANGUAGES` and nothing else. A language nobody tells the model about is a picker
/// that silently does nothing.
#[test]
fn every_language_in_the_table_reaches_the_model_by_name() {
    let p = ep1();
    for l in lang::LANGUAGES {
        let s = brief(&p, "Critical", 84.0, l);
        assert!(
            s.contains(&format!("in first person, in {},", l.speaks)),
            "{}: the brief never names the language",
            l.id
        );
        // The case is the case, whoever is speaking.
        assert!(s.contains("allergic to shrimp"), "{}: the case changed", l.id);
        assert!(s.contains("84 percent"), "{}: her observations changed", l.id);
    }
}

/// A reply that came back in the wrong alphabet is *noticed*, not swallowed. The learner is told;
/// the answer is still shown, because it is still her answer and still true about the case.
#[test]
fn a_reply_in_the_wrong_language_is_detected_but_never_destroyed() {
    let th = lang::language(Some("th"));
    assert!(!lang::reply_is_in(th, "I can't breathe. My throat is closing."));
    assert!(lang::reply_is_in(th, "หายใจไม่ออก… คอมันตีบ"));
    // Mixed is fine: a Thai sentence carrying "EpiPen" has still answered in Thai.
    assert!(lang::reply_is_in(th, "หนูมี EpiPen แต่ลืมไว้ที่บ้าน"));
}
