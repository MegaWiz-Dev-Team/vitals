//! The catalogue, as the factory reads it.
//!
//! The ward says which sixteen cases it serves (`vitals_web::ward::CATALOGUE`), what level each is
//! (`difficulty_of`), and — since 949a76b — who each station was written about (`case_patient`,
//! the door's own reading of `demo/personas/<id>.json`) and how far a pack's age may sit from
//! that (`age_band`). The door refuses a pack that contradicts any of it. So the factory does not
//! have a rule of its own here: it asks the ward's functions, and reads a file itself only where
//! the door has none baked in.
//!
//! A case no file describes is not built. The alternative — a guess — is exactly the pack the
//! door takes at its word: the four episodes carry no persona file, and a pack for one of them
//! is checked by nobody.

use std::path::PathBuf;
use vitals_factory::catalogue::{read_case, read_catalogue, Sex};
use vitals_web::ward::{age_band, case_patient, difficulty_of, CATALOGUE};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A persona file, as `demo/personas/<id>.json` is shaped.
fn persona(sex: &str, age: u16) -> String {
    format!(r#"{{"id":"x","patient":{{"name":"Somchai","age":{age},"sex":"{sex}","affect":"calm"}}}}"#)
}

const BARE_SCENARIO: &str = r#"{"_note":"MOCK","setting":"ED","vitals0":{"hr":90}}"#;

/// Every id the ward serves is accounted for, one way or the other, and the level is the ward's.
#[test]
fn every_case_the_ward_serves_is_either_buildable_or_says_why_not() {
    let cat = read_catalogue(&repo_root());
    let mut seen: Vec<&str> = cat.cases.iter().map(|c| c.id.as_str()).collect();
    seen.extend(cat.unbuildable.iter().map(|u| u.id.as_str()));
    seen.sort_unstable();
    let mut want: Vec<&str> = CATALOGUE.to_vec();
    want.sort_unstable();
    assert_eq!(seen, want, "the factory reads exactly the ward's catalogue, nothing more or less");
    for c in &cat.cases {
        assert_eq!(Some(c.difficulty), difficulty_of(&c.id), "{}: the level is the ward's own", c.id);
        assert!(!c.source.is_empty(), "{}: says where her sex and age came from", c.id);
    }
    for u in &cat.unbuildable {
        assert!(!u.why.is_empty(), "{}: a case that is not built says why", u.id);
    }
    // The twelve stations are the ones the door has a patient for; as the files stand today the
    // four episodes have none, and the factory says so rather than guessing.
    for id in CATALOGUE {
        match case_patient(id) {
            Some(_) => assert!(cat.cases.iter().any(|c| c.id == id), "{id}: the door knows her, so she builds"),
            None => assert!(cat.unbuildable.iter().any(|u| u.id == id), "{id}: nobody states her sex and age"),
        }
    }
}

/// A station's sex and band are exactly the door's: the same reading of the same file, and the
/// same distance from the authored age. Anything else is a pack the door refuses.
#[test]
fn a_stations_sex_and_band_are_the_doors_own() {
    let cat = read_catalogue(&repo_root());
    for id in CATALOGUE.iter().filter(|c| c.starts_with("osce-")) {
        let theirs = case_patient(id).expect("a station has a patient");
        let ours = cat.get(id).unwrap_or_else(|| panic!("{id} builds"));
        assert_eq!(ours.sex.letter(), theirs.sex, "{id}");
        assert_eq!(ours.band, age_band(theirs.age), "{id}");
        assert!(ours.source.contains(&format!("demo/personas/{id}.json")), "{id}: source: {}", ours.source);
    }
    let a = cat.get("osce-a").unwrap();
    assert_eq!((a.sex, a.band.clone()), (Sex::M, 64..=78), "M 71, a tenth either side");
    let b3 = cat.get("osce-b3").unwrap();
    assert_eq!((b3.sex, b3.band.clone()), (Sex::F, 1..=5), "F 3, the floor of two years, never below one");
}

/// Where the door has no patient baked in, the factory reads the persona file the way the door
/// reads its own — so if one appears for an episode, she is read the same way the stations are.
#[test]
fn a_persona_file_the_door_has_not_baked_in_is_read_the_way_the_door_reads_them() {
    assert!(case_patient("ep2").is_none(), "this test is about the gap");
    let c = read_case("ep2", BARE_SCENARIO, Some(&persona("M", 58))).expect("the file builds him");
    assert_eq!(c.sex, Sex::M);
    assert_eq!(c.band, age_band(58));
    assert!(c.source.contains("demo/personas/ep2.json"), "{}", c.source);
    let why = read_case("ep2", BARE_SCENARIO, Some(&persona("x", 40))).expect_err("x is nobody").why;
    assert!(why.contains("sex"), "{why}");
}

/// A scenario may also carry a `ward` block naming sex and band outright, for a case with no
/// persona file. It is read only then: a station's block that disagreed with its persona file
/// would be a pack the door refuses, so the file the door reads wins.
#[test]
fn a_ward_block_in_the_scenario_serves_a_case_with_no_persona_file() {
    let sce = r#"{"_note":"MOCK","ward":{"sex":"f","age":[30,40]},"setting":"ED"}"#;
    let c = read_case("ep4", sce, None).expect("the block builds her");
    assert_eq!(c.sex, Sex::F);
    assert_eq!(c.band, 30..=40);
    assert!(c.source.contains("ward"), "source names the block: {}", c.source);

    // A single number is widened the way the door widens an authored age.
    let sce = r#"{"ward":{"sex":"m","age":9}}"#;
    let c = read_case("ep3", sce, None).expect("a number is an authored age");
    assert_eq!(c.band, age_band(9));

    // Against a persona file, the file wins — it is what the door checks.
    let c = read_case("ep2", sce, Some(&persona("F", 40))).expect("builds");
    assert_eq!((c.sex, c.band.clone()), (Sex::F, age_band(40)));
}

/// No block, no persona file: not built, and the reason names what is missing.
#[test]
fn a_case_no_file_describes_is_not_built() {
    let why = read_case("ep2", BARE_SCENARIO, None).expect_err("nothing states her sex or age").why;
    assert!(why.contains("sex") && why.contains("age"), "says what is missing: {why}");
    assert!(why.contains("demo/personas/ep2.json"), "and where it would be read from: {why}");
}

/// The practice case has no level on the ward, so it has no place in a pack.
#[test]
fn a_case_the_ward_gives_no_level_is_not_built() {
    let why = read_case("ep1", BARE_SCENARIO, Some(&persona("F", 30))).expect_err("ep1 is practice").why;
    assert!(why.contains("level"), "{why}");
}

/// Sex is one of two letters, in either case, and nothing else is guessed at.
#[test]
fn sex_is_read_in_either_case_and_nothing_else() {
    assert_eq!(Sex::parse("M"), Some(Sex::M));
    assert_eq!(Sex::parse("f"), Some(Sex::F));
    assert_eq!(Sex::parse(" F "), Some(Sex::F));
    assert_eq!(Sex::parse("female"), None, "a word is not a code the pool uses");
    assert_eq!(Sex::parse("x"), None);
    assert_eq!(Sex::F.letter(), "f");
    assert_eq!(Sex::M.word(), "man");
}
