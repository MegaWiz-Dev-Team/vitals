//! The catalogue, as the factory reads it.
//!
//! The ward says which sixteen cases it serves (`vitals_web::ward::CATALOGUE`) and what level each
//! is (`difficulty_of`). What the ward cannot say — and says so in `validate_pack` — is the sex
//! and the age band the case was written for, because those live in the case's own files. The
//! factory reads them from there, and a case whose files state neither is not built: a pack that
//! guessed would put a woman's name on a man's presentation, and the door cannot catch it.

use std::path::PathBuf;
use vitals_factory::catalogue::{band_around, read_case, read_catalogue, Sex};
use vitals_web::ward::{difficulty_of, CATALOGUE};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A station's persona file, as `demo/personas/<id>.json` is shaped today.
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
    // The twelve stations carry a persona file, so they are buildable as the files are today.
    for id in CATALOGUE.iter().filter(|c| c.starts_with("osce-")) {
        assert!(cat.cases.iter().any(|c| &c.id == id), "{id} has a persona file and should build");
    }
}

/// `demo/personas/osce-a.json` says M 71; `osce-b3.json` says F 3. The band is around the authored
/// age and on the same side of childhood.
#[test]
fn a_stations_sex_and_age_come_from_its_persona_file() {
    let cat = read_catalogue(&repo_root());
    let a = cat.cases.iter().find(|c| c.id == "osce-a").expect("osce-a builds");
    assert_eq!(a.sex, Sex::M);
    assert!(a.band.contains(&71), "the authored age is inside the band");
    assert!(*a.band.start() >= 18, "an adult case stays adult");
    assert!(a.source.contains("demo/personas/osce-a.json"), "source: {}", a.source);

    let b3 = cat.cases.iter().find(|c| c.id == "osce-b3").expect("osce-b3 builds");
    assert_eq!(b3.sex, Sex::F);
    assert!(b3.band.contains(&3));
    assert!(*b3.band.end() <= 17, "a child's case stays a child's: {:?}", b3.band);
    assert!(*b3.band.start() >= 1);
}

/// A scenario may carry a `ward` block naming sex and band outright. When it does, it wins — it
/// is the case saying who its patient is, and the persona file is the bay's rendering of one of
/// them.
#[test]
fn a_ward_block_in_the_scenario_wins_over_the_persona_file() {
    let sce = r#"{"_note":"MOCK","ward":{"sex":"f","age":[30,40]},"setting":"ED"}"#;
    let c = read_case("osce-a", sce, Some(&persona("M", 71))).expect("the block builds her");
    assert_eq!(c.sex, Sex::F);
    assert_eq!(c.band, 30..=40);
    assert!(c.source.contains("ward"), "source names the block: {}", c.source);

    // A single number is a band of one.
    let sce = r#"{"ward":{"sex":"m","age":9}}"#;
    let c = read_case("osce-b2", sce, None).expect("a number is a band of one");
    assert_eq!(c.band, 9..=9);
}

/// No block, no persona file: not built, and the reason names what is missing.
#[test]
fn a_case_no_file_describes_is_not_built() {
    let why = read_case("ep2-stemi", BARE_SCENARIO, None).expect_err("nothing states her sex or age").why;
    assert!(why.contains("sex") && why.contains("age"), "says what is missing: {why}");
    assert!(why.contains("demo/personas/ep2-stemi.json") || why.contains("ward"), "and where it would be read from: {why}");
}

/// The practice case has no level on the ward, so it has no place in a pack.
#[test]
fn a_case_the_ward_gives_no_level_is_not_built() {
    let why = read_case("ep1", BARE_SCENARIO, Some(&persona("F", 30))).expect_err("ep1 is practice").why;
    assert!(why.contains("level") || why.contains("difficulty"), "{why}");
}

/// Sex is one of two letters, in either case, and nothing else is guessed at.
#[test]
fn sex_is_read_in_either_case_and_nothing_else() {
    assert_eq!(Sex::parse("M"), Some(Sex::M));
    assert_eq!(Sex::parse("f"), Some(Sex::F));
    assert_eq!(Sex::parse(" F "), Some(Sex::F));
    assert_eq!(Sex::parse("female"), None, "a word is not a code the pool uses");
    assert_eq!(Sex::parse("x"), None);
    let why = read_case("osce-a", BARE_SCENARIO, Some(&persona("x", 40))).expect_err("x is nobody").why;
    assert!(why.contains("sex"), "{why}");
    assert_eq!(Sex::F.letter(), "f");
    assert_eq!(Sex::M.word(), "man");
}

/// The band around an authored age: wide enough that the sixty faces already made (at 28, 45 and
/// 63) fit every adult case, narrow enough that a child stays a child and a 25-year-old is not 45.
#[test]
fn a_band_around_an_authored_age_keeps_her_the_same_kind_of_patient() {
    for (authored, face) in [(71, 63), (68, 63), (72, 63), (62, 63), (55, 63), (53, 45), (25, 28)] {
        let band = band_around(authored);
        assert!(band.contains(&authored));
        assert!(band.contains(&face), "a face made at {face} serves a case written at {authored}: {band:?}");
    }
    assert!(!band_around(25).contains(&45), "a 25-year-old is not 45");
    assert!(!band_around(71).contains(&45), "a 71-year-old is not 45");
    for child in [3, 6, 14] {
        let band = band_around(child);
        assert!(band.contains(&child));
        assert!(*band.start() >= 1 && *band.end() <= 17, "{child}: {band:?}");
    }
    assert!(*band_around(18).start() >= 18, "an adult case never dips into childhood");
    assert!(*band_around(120).end() <= 120);
}
