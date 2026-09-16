//! Which case a patient presents, chosen from the ward's own list.
//!
//! The ward has a case door (`GET /api/ward/cases`, 7b, 16 Sep 2026) that lists every case it
//! holds — case_id, country or null, difficulty, endemic, provisional, version — and its queue door
//! is about to read an optional `case_id` and `difficulty` on a pack. The factory chooses one for
//! every pack it builds or re-sends: an endemic case for the patient's country when one exists
//! and her age and sex fit its band, else any case with no country at the difficulty the queue
//! is short of (student, intern and resident kept about 1:1:1 across the queue — the founder's
//! rule that difficulty levels exist so a stranger can choose); never an endemic case of another
//! country; never a case already in a bed; and, until the board publishes a case_id per bed, as
//! few duplicates within the queue as the fit allows. A case whose band the factory does not know
//! is never chosen: an age that fits is a claim the factory can only make about a case it has.

use std::collections::{BTreeMap, BTreeSet};
use vitals_factory::cases::{choose, Chosen, Mix, Patient};
use vitals_factory::catalogue::{Case, Catalogue, Sex};
use vitals_factory::door::{parse_cases, push_body, Outbound, WardCase};
use vitals_web::ward::{age_band, case_patient, difficulty_of, Pack, Persona};

const CASES: &str = include_str!("fixtures/ward-cases-2026-09-16.json");

fn case(id: &str) -> Case {
    let theirs = case_patient(id).expect("a station");
    Case { id: id.into(), sex: Sex::parse(&theirs.sex).unwrap(), band: age_band(theirs.age), difficulty: difficulty_of(id).expect("a ward case"), source: "test".into() }
}

/// The twelve stations, and one community case the factory has a file for: dengue, a woman of
/// 20–40, intern, endemic in Thailand.
fn catalogue() -> Catalogue {
    let mut cases: Vec<Case> = ["osce-a", "osce-a2", "osce-b", "osce-b2", "osce-b3", "osce-c", "osce-c2", "osce-c3", "osce-d", "osce-d2", "osce-d3", "osce-d4"].iter().map(|id| case(id)).collect();
    cases.push(Case { id: "cmty-th-dengue-01".into(), sex: Sex::F, band: 20..=40, difficulty: "intern", source: "test".into() });
    Catalogue { cases, unbuildable: vec![] }
}

fn ward_cases() -> Vec<WardCase> {
    parse_cases(CASES).expect("the fixture parses")
}

fn patient<'a>(country: &'a str, sex: Sex, age: u16, own: &'a str) -> Patient<'a> {
    Patient { country, sex, age, own_case: own }
}

fn nobody() -> BTreeSet<String> {
    BTreeSet::new()
}

#[test]
fn the_case_door_lists_cases_in_either_shape_and_every_field_is_read() {
    let cases = ward_cases();
    assert_eq!(cases.len(), 18);
    let a = cases.iter().find(|c| c.case_id == "osce-a").unwrap();
    assert_eq!((a.country.as_deref(), a.difficulty.as_str(), a.endemic, a.provisional), (None, "student", false, false));
    let dengue = cases.iter().find(|c| c.case_id == "cmty-th-dengue-01").unwrap();
    assert_eq!((dengue.country.as_deref(), dengue.difficulty.as_str(), dengue.endemic, dengue.provisional), (Some("THA"), "intern", true, true));
    assert_eq!(dengue.version, Some(serde_json::json!(3)));
    // A bare array is the same list.
    let bare = serde_json::to_string(&cases).unwrap();
    assert_eq!(parse_cases(&bare).unwrap(), cases);
    // A version that is a string is still a version; a missing one is none.
    let odd = parse_cases(r#"{"cases":[{"case_id":"x","difficulty":"intern","version":"2026-09-16"}]}"#).unwrap();
    assert_eq!((odd[0].country.as_deref(), odd[0].endemic, odd[0].version.clone()), (None, false, Some(serde_json::json!("2026-09-16"))));
    assert!(parse_cases("not json").is_err());
    assert!(parse_cases(r#"{"the_ward_is":"https://elsewhere"}"#).is_err(), "another host is not a case list");
}

#[test]
fn an_endemic_case_of_her_country_comes_first_when_her_age_and_sex_fit_it() {
    let (cases, cat) = (ward_cases(), catalogue());
    // A Thai woman of 26 on osce-c3 (F 25, intern): dengue is listed for THA, she fits 20–40.
    let c = choose(&cases, &cat, &patient("THA", Sex::F, 26, "osce-c3"), &nobody(), &Mix::default(), 1, 0).expect("a case");
    assert_eq!((c.case_id.as_str(), c.difficulty.as_str()), ("cmty-th-dengue-01", "intern"));
    assert!(c.why.contains("endemic"), "{}", c.why);
    // A Thai man of 25 does not fit a case written for a woman: no endemic case, so the ordinary
    // draw.
    let c = choose(&cases, &cat, &patient("THA", Sex::M, 25, "osce-b"), &nobody(), &Mix::default(), 1, 0).expect("a case");
    assert_ne!(c.case_id, "cmty-th-dengue-01");
    assert!(!c.why.contains("endemic"), "{}", c.why);
    // A Thai woman of 66 (osce-a2) is outside the band: the ordinary draw.
    let c = choose(&cases, &cat, &patient("THA", Sex::F, 66, "osce-a2"), &nobody(), &Mix::default(), 1, 0).expect("a case");
    assert_ne!(c.case_id, "cmty-th-dengue-01");
}

#[test]
fn an_endemic_case_of_another_country_is_never_chosen_and_an_unknown_band_is_never_guessed() {
    let (cases, cat) = (ward_cases(), catalogue());
    // An Indonesian woman of 26 fits dengue's band, and dengue is Thailand's here.
    for seed in 0..20 {
        let c = choose(&cases, &cat, &patient("IDN", Sex::F, 26, "osce-c3"), &nobody(), &Mix::default(), seed, 0).expect("a case");
        assert_ne!(c.case_id, "cmty-th-dengue-01", "seed {seed}: an endemic case of another country");
        assert!(cases.iter().find(|w| w.case_id == c.case_id).is_some_and(|w| !w.endemic && w.country.is_none()), "{c:?}");
    }
    // Nepal's altitude case is listed and endemic for NPL, but the factory has no file for it, so
    // it does not know the band: a Nepali woman gets an ordinary case, never a guess.
    let c = choose(&cases, &cat, &patient("NPL", Sex::F, 26, "osce-c3"), &nobody(), &Mix::default(), 1, 0).expect("a case");
    assert_ne!(c.case_id, "cmty-np-altitude-01");
    // Nor are the four episodes chosen: listed by the ward, but no file states their patient.
    for seed in 0..20 {
        let c = choose(&cases, &cat, &patient("KEN", Sex::F, 55, "osce-d2"), &nobody(), &Mix::default(), seed, 0).expect("a case");
        assert!(!c.case_id.starts_with("ep"), "{c:?}");
    }
}

#[test]
fn a_case_in_a_bed_is_never_chosen_and_nothing_fitting_is_none_not_a_guess() {
    let (cases, cat) = (ward_cases(), catalogue());
    // A girl of 6: osce-c (F 6, resident) and osce-d3 (F 6, intern) fit her and nothing else.
    let c = choose(&cases, &cat, &patient("KEN", Sex::F, 6, "osce-c"), &nobody(), &Mix::default(), 1, 0).expect("a case");
    assert!(["osce-c", "osce-d3"].contains(&c.case_id.as_str()), "{c:?}");
    let board: BTreeSet<String> = ["osce-c".to_string()].into_iter().collect();
    let c = choose(&cases, &cat, &patient("KEN", Sex::F, 6, "osce-c"), &board, &Mix::default(), 1, 0).expect("a case");
    assert_eq!(c.case_id, "osce-d3", "osce-c is in a bed");
    let both: BTreeSet<String> = ["osce-c".to_string(), "osce-d3".to_string()].into_iter().collect();
    assert_eq!(choose(&cases, &cat, &patient("KEN", Sex::F, 6, "osce-c"), &both, &Mix::default(), 1, 0), None, "nothing fits her that is not in a bed");
    // No list at all: no choice, and no guess.
    assert_eq!(choose(&[], &cat, &patient("KEN", Sex::F, 6, "osce-c"), &nobody(), &Mix::default(), 1, 0), None);
}

#[test]
fn the_difficulty_follows_the_queues_mix_and_her_own_case_wins_ties() {
    let (cases, cat) = (ward_cases(), catalogue());
    // A woman of 55 fits osce-c2 (F 53, intern), osce-d2 (F 55, resident) and nothing else.
    let who = patient("BRA", Sex::F, 55, "osce-d2");
    // An empty queue: her own case, resident.
    let c = choose(&cases, &cat, &who, &nobody(), &Mix::default(), 1, 0).unwrap();
    assert_eq!((c.case_id.as_str(), c.difficulty.as_str()), ("osce-d2", "resident"));
    assert!(c.why.contains("own"), "{}", c.why);
    // A queue heavy in residents: the intern case, even though osce-d2 is her own.
    let mut mix = Mix::default();
    for _ in 0..6 {
        mix.count("resident", "osce-d4");
    }
    let c = choose(&cases, &cat, &who, &nobody(), &mix, 1, 0).unwrap();
    assert_eq!((c.case_id.as_str(), c.difficulty.as_str()), ("osce-c2", "intern"));
    assert!(c.why.contains("intern"), "{}", c.why);
    // Equal mix, but osce-d2 is already twice in the queue and osce-c2 not at all: fewer
    // duplicates wins over her own case, since the board does not yet say which case a bed holds.
    let mut mix = Mix::default();
    mix.count("resident", "osce-d2");
    mix.count("resident", "osce-d2");
    mix.count("intern", "osce-b");
    mix.count("intern", "osce-b2");
    let c = choose(&cases, &cat, &who, &nobody(), &mix, 1, 0).unwrap();
    assert_eq!(c.case_id, "osce-c2", "{c:?}");
    // The mix counts what it was told and nothing else.
    assert_eq!((mix.of("resident"), mix.of("intern"), mix.of("student")), (2, 2, 0));
    assert_eq!(mix.queued("osce-d2"), 2);
}

#[test]
fn the_choice_is_the_same_for_the_same_seed_and_slot_and_moves_with_them() {
    let (cases, cat) = (ward_cases(), catalogue());
    // A man of 25 fits osce-b (M 25) alone among the stations — so the seed cannot move him.
    let who = patient("EGY", Sex::M, 25, "osce-b");
    let a = choose(&cases, &cat, &who, &nobody(), &Mix::default(), 3, 0).unwrap();
    let b = choose(&cases, &cat, &who, &nobody(), &Mix::default(), 3, 0).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.case_id, "osce-b");
    // A woman of 72 fits osce-a2 (F 68, student) and osce-d4 (F 72, resident): with an empty
    // queue the student and resident levels tie, her own case wins, and the seed decides nothing.
    let who = patient("EGY", Sex::F, 72, "osce-d4");
    let seeds: BTreeSet<String> = (0..10).map(|s| choose(&cases, &cat, &who, &nobody(), &Mix::default(), s, 0).unwrap().case_id).collect();
    assert_eq!(seeds.into_iter().collect::<Vec<_>>(), vec!["osce-d4"]);
    let _: Chosen = a;
}

#[test]
fn a_pack_goes_through_the_door_with_its_case_and_difficulty_and_without_them_when_none_was_chosen() {
    let pack = Pack {
        case: "osce-a2".into(),
        persona: Persona { name: "Tigist Alemu".into(), age: 66, country: "ETH".into(), sex: "f".into() },
        portrait: BTreeMap::new(),
        endemic: false,
    };
    let with = Outbound { pack: pack.clone(), case_id: Some("osce-a2".into()), difficulty: Some("student".into()) };
    let body: serde_json::Value = serde_json::from_str(&push_body(std::slice::from_ref(&with))).unwrap();
    let p = &body["packs"][0];
    assert_eq!(p["case"], "osce-a2");
    assert_eq!(p["case_id"], "osce-a2");
    assert_eq!(p["difficulty"], "student");
    assert_eq!(p["persona"]["name"], "Tigist Alemu");
    assert_eq!(p["endemic"], false);
    // The door of today reads the pack's own fields and ignores the two it does not know; the
    // pack is still the pack the door content-addresses.
    let plain: Pack = serde_json::from_value(p.clone()).unwrap();
    assert_eq!(plain, pack);
    let without = Outbound::plain(pack.clone());
    let body: serde_json::Value = serde_json::from_str(&push_body(std::slice::from_ref(&without))).unwrap();
    assert!(body["packs"][0].get("case_id").is_none() && body["packs"][0].get("difficulty").is_none(), "{body}");
}
