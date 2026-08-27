//! The patient who answers is the patient in the bed.
//!
//! For as long as the bay has had a voice, it had exactly one. `Patient::connect` read
//! `demo/ep1-en.json` once at boot and every session in the season borrowed the result, so asking
//! the seventy-one-year-old man at OSCE A about his chest got an answer from a nineteen-year-old
//! woman — her name, her age, her shrimp allergy, her salad — while the monitor above her bed said
//! Somchai, M 71. The `sex` field the personas carry was real and correct and reached nothing: the
//! brief was faithful to a patient nobody was playing.
//!
//! These tests read the shipped persona files and the brief that would go on the wire. No gateway,
//! no model, no network — the thing that has to be true is that the *right character leaves the
//! building*, and that has to be checkable on a laptop in CI on every commit.

use std::collections::BTreeSet;
use std::path::PathBuf;
use vitals_web::lang;
use vitals_web::patient::brief;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every station persona on disk, as (id, json). EP1 lives at its own path and is checked apart.
fn personas() -> Vec<(String, serde_json::Value)> {
    let dir = root().join("demo/personas");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let id = p.file_stem().unwrap().to_string_lossy().to_string();
            let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{id}: {e}"));
            (id.clone(), serde_json::from_str(&text).unwrap_or_else(|e| panic!("{id}: {e}")))
        })
        .collect()
}

fn ep1() -> serde_json::Value {
    let p = root().join("demo/ep1-en.json");
    serde_json::from_str(&std::fs::read_to_string(&p).expect("EP1's text layer")).expect("JSON")
}

/// The twelve stations the season ships all have a voice, and each is its own person.
///
/// Named rather than counted: a persona file quietly deleted would pass a length check by
/// deleting the expectation with it.
#[test]
fn every_station_has_its_own_persona() {
    let have: BTreeSet<String> = personas().into_iter().map(|(id, _)| id).collect();
    for id in [
        "osce-a", "osce-a2", "osce-b", "osce-b2", "osce-b3", "osce-c", "osce-c2", "osce-c3",
        "osce-d", "osce-d2", "osce-d3", "osce-d4",
    ] {
        assert!(have.contains(id), "{id} has no persona — its patient would play mute");
    }
}

/// No two cases share a patient. This is the bug, stated as a property: seventeen sessions used to
/// resolve to one name, one age and one allergy.
#[test]
fn no_two_cases_are_played_by_the_same_person() {
    let mut seen: Vec<(String, String)> = vec![("ep1".into(), "Ing".into())];
    for (id, p) in personas() {
        let name = p["patient"]["name"].as_str().unwrap_or_default().to_string();
        assert!(!name.is_empty(), "{id}: a persona with no name");
        if let Some((other, _)) = seen.iter().find(|(_, n)| *n == name) {
            panic!("{id} and {other} are both played by {name}");
        }
        seen.push((id, name));
    }
}

/// Each persona says what the shelf card says. The season table in `static/index.html` is what the
/// player bar and the monitor banner print — "Somchai · M 71" — so a persona that disagrees puts a
/// different age or a different sex in the patient's own mouth than on the screen above the bed.
/// That is a wrong-patient error, and it is the first thing a clinician in the room notices.
#[test]
fn a_persona_agrees_with_the_bed_label() {
    // (id, name, sex, age) — read off SEASON in static/index.html, which is the copy the bay
    // paints. Kept here by hand on purpose: if somebody re-ages a case in one place, this fails.
    let bed = [
        ("osce-a", "Somchai", "M", 71),
        ("osce-a2", "Somsri", "F", 68),
        ("osce-b", "Somchai Jaidee", "M", 25),
        ("osce-b2", "Tan", "M", 14),
        ("osce-b3", "Pim", "F", 3),
        ("osce-c", "Fon", "F", 6),
        ("osce-c2", "Wasana", "F", 53),
        ("osce-c3", "Waen", "F", 25),
        ("osce-d", "Somchai Jaiman", "M", 62),
        ("osce-d2", "Somsri Jaidee", "F", 55),
        ("osce-d3", "Beam", "F", 6),
        ("osce-d4", "Pranom", "F", 72),
    ];
    let have = personas();
    for (id, name, sex, age) in bed {
        let (_, p) = have.iter().find(|(k, _)| k == id).unwrap_or_else(|| panic!("{id} missing"));
        assert_eq!(p["patient"]["name"].as_str(), Some(name), "{id}: not the name on the bed");
        assert_eq!(p["patient"]["sex"].as_str(), Some(sex), "{id}: not the sex on the bed");
        assert_eq!(p["patient"]["age"].as_i64(), Some(age), "{id}: not the age on the bed");
    }
}

/// The brief handed to the model names *this* patient, in *these* pronouns, and never EP1's.
///
/// The tells are EP1's own sentences, not her diagnosis: two stations are anaphylaxis after
/// seafood and are entitled to their own prawns. What no station may carry is the salad she
/// ordered, the pen she left at home, or her name.
#[test]
fn the_brief_is_about_the_case_and_never_about_ep1() {
    for (id, p) in personas() {
        let s = brief(&p, "Deteriorating", 91.0, lang::default_language());
        let name = p["patient"]["name"].as_str().unwrap();
        assert!(s.contains(name), "{id}: the brief never names {name}");
        for ep1_only in ["You are Ing,", "asked for no shrimp", "left it at home", "red welts all over"] {
            assert!(!s.contains(ep1_only), "{id}: EP1 leaked into the brief — \"{ep1_only}\"");
        }

        // The sex the case states is the sex the brief speaks in — the fix that had no effect
        // while one persona was shared, because the shared one was always a woman.
        let male = p["patient"]["sex"].as_str() == Some("M");
        let carer = p.get("speaker").is_some();
        if !carer {
            assert_eq!(
                s.contains("Speak ONLY as him,"),
                male,
                "{id}: the brief speaks in the wrong pronoun"
            );
        }
    }
}

/// A case where somebody else does the talking says so, in as many words, and tells the model not
/// to be the patient. Three of the twelve stations are children, and a three-year-old with stridor
/// does not answer questions about her own rhinorrhoea.
#[test]
fn a_child_is_spoken_for_and_never_speaks() {
    for id in ["osce-b3", "osce-c", "osce-d3"] {
        let (_, p) = personas().into_iter().find(|(k, _)| k == id).expect(id);
        let sp = p.get("speaker").unwrap_or_else(|| panic!("{id}: a small child with no informant"));
        let voice = sp["name"].as_str().expect("the informant is named");
        let child = p["patient"]["name"].as_str().unwrap();

        let s = brief(&p, "Deteriorating", 91.0, lang::default_language());
        assert!(s.contains(&format!("Speak ONLY as {voice}")), "{id}: {voice} is not the voice");
        assert!(s.contains(&format!("never as {child}")), "{id}: nothing stops it playing {child}");
        // The observations belong to the child, not to the adult reporting them.
        assert!(s.contains(&format!("{child} is Deteriorating")), "{id}: whose deterioration?");
        assert!(!s.contains("your oxygen saturation"), "{id}: the carer is the one desaturating");
    }
}

/// An adult patient is still the one talking. The carer branch must not leak into the other nine.
#[test]
fn an_adult_speaks_for_themselves() {
    for (id, p) in personas() {
        if p.get("speaker").is_some() {
            continue;
        }
        let s = brief(&p, "Critical", 84.0, lang::default_language());
        assert!(s.contains("Right now you are Critical"), "{id}: somebody else took the voice");
        assert!(s.contains("your oxygen saturation is 84 percent"), "{id}: not their own numbers");
    }
}

/// Every persona is a complete brief: dialogue to be truthful about, and a line for the moment it
/// cannot answer. A persona with an empty script is a model with nothing to be faithful to, which
/// is the state that invents an allergy inside one turn.
#[test]
fn every_persona_carries_a_script_and_a_way_out() {
    for (id, p) in personas() {
        let d = p["dialogue"].as_array().unwrap_or_else(|| panic!("{id}: no dialogue"));
        assert!(d.len() >= 6, "{id}: {} lines is not a history", d.len());
        assert!(
            d.iter().any(|n| n["reveal"] == "volunteered"),
            "{id}: nothing is volunteered — the patient opens with silence"
        );
        assert!(
            d.iter().any(|n| n["reveal"] == "on_direct_ask"),
            "{id}: nothing is held back, so taking a history earns nothing"
        );
        for n in d {
            let nid = n["id"].as_str().unwrap_or("<unnamed>");
            assert!(!nid.is_empty(), "{id}: a dialogue node with no id");
            let line = n["patient"].as_str().unwrap_or_default();
            assert!(!line.trim().is_empty(), "{id}/{nid}: a node with nothing to say");
            assert!(
                ["volunteered", "on_ask", "on_direct_ask"].contains(&n["reveal"].as_str().unwrap_or("")),
                "{id}/{nid}: reveal '{}' is not one the gate knows",
                n["reveal"]
            );
        }
        assert!(
            p["fallback"].as_str().is_some_and(|f| !f.trim().is_empty()),
            "{id}: no fallback line for the turn it cannot answer"
        );
        // Provenance is not decoration here: every one of these was derived from a case in
        // embla-cases and a reader has to be able to get back to it.
        assert!(p["case"].as_str().is_some(), "{id}: no source case recorded");
        assert!(
            p["_note"].as_str().is_some_and(|n| n.contains("sha256:")),
            "{id}: the note does not pin which version of the source it came from"
        );
    }
}

/// EP1's brief did not move. Every new field defaults to the sentence it already produced, so the
/// case that was right before this file learned about seventeen patients is still byte-for-byte
/// right — including under a language the notes are not written in.
#[test]
fn ep1_is_untouched_by_all_of_this() {
    let p = ep1();
    let s = brief(&p, "Deteriorating", 91.0, lang::default_language());
    assert!(s.starts_with("You are Ing, 19 years old, in an emergency department right now. \
                           You are frightened and short of breath. Speak ONLY as her, in first \
                           person, in English, in one or two short sentences. Broken, breathless \
                           phrasing."));
    assert!(s.contains("What is true about you, and what you say if asked:"));
    assert!(s.contains("Right now you are Deteriorating and your oxygen saturation is 91 percent"));
    assert!(s.contains("allergic to shrimp"), "EP1's notes changed");

    let th = brief(&p, "Deteriorating", 91.0, lang::language(Some("th")));
    assert!(th.contains("in first person, in Thai,"));
    assert!(th.contains("LANGUAGE."));
}
