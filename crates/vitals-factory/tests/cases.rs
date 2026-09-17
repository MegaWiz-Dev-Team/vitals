//! The ward's list is the catalogue.
//!
//! Coordinator, 16 Sep 2026, after a dry run against staging: the queue door refuses season ids,
//! so the factory's season catalogue goes; a pack's `case` is a World case_id from
//! `GET /api/ward/cases` — `{cases: [{archetype, case_id, country, difficulty, endemic,
//! provisional, title, version, patient?}], derivations: {…}}` — and the case is chosen first,
//! then a person of the case's sex. `patient: {age, sex}` (sex spelled male/female) is the ward
//! owner's addition; a row without it fits any adult, never "no case".
//!
//! Fit: the sex must match hers; her age within 12 years of the case's patient; a case under 16
//! takes a persona under 16 only, and a case of 16 or more a persona of 16 or more.
//!
//! Endemic: the factory's knowledge of what is endemic where is the list itself — a country "has
//! an endemic list" iff a placeable case is tagged endemic for it — and the founder's one draw in
//! five ([`vitals_web::ward::ENDEMIC_IN`]) takes such a case; the other four are the common draw.
//! When nothing of the common draw is placeable at all, the case written for home is the only
//! case there is and is taken outside the draw. A withdrawn row is never chosen, whatever else
//! it says. No file is read for any of this.

use std::collections::BTreeSet;
use vitals_factory::cases::{age_window, endemic_draw, fits, has_endemic_list, placeable, rank, sex_of, Mix, ADULT_ANY, AGE_SLACK, CHILD_UNDER};
use vitals_web::ward::ENDEMIC_IN;
use vitals_factory::door::{parse_cases, WardCase};
use vitals_factory::sex::Sex;

const CASES: &str = include_str!("fixtures/ward-cases-2026-09-16.json");

fn cases() -> Vec<WardCase> {
    parse_cases(CASES).expect("the fixture parses")
}

/// The fixture's placeable rows: everything but the withdrawn one.
fn live() -> Vec<WardCase> {
    placeable(&cases())
}

fn by_id<'a>(cases: &'a [WardCase], id: &str) -> &'a WardCase {
    cases.iter().find(|c| c.case_id == id).unwrap_or_else(|| panic!("{id}"))
}

fn nobody() -> BTreeSet<String> {
    BTreeSet::new()
}

fn ids<'a>(ranked: &[(&'a WardCase, String)]) -> Vec<&'a str> {
    ranked.iter().map(|(c, _)| c.case_id.as_str()).collect()
}

#[test]
fn the_case_door_is_parsed_in_the_real_shape_with_and_without_a_patient() {
    let cases = cases();
    assert_eq!(cases.len(), 19, "eighteen placeable and one withdrawn");
    assert_eq!(live().len(), 18);
    assert!(cases.iter().any(|c| c.withdrawn && c.case_id == "world-withdrawn-angina"));
    assert!(live().iter().all(|c| !c.withdrawn));
    let acs = by_id(&cases, "world-acs-elderly-man");
    assert_eq!((acs.archetype.as_deref(), acs.country.as_deref(), acs.difficulty.as_str(), acs.endemic, acs.provisional), (Some("chest-pain"), None, "student", false, false));
    assert_eq!(acs.title.as_deref(), Some("Chest pain in an older man"));
    assert_eq!(acs.version, Some(serde_json::json!(1)));
    let p = acs.patient.as_ref().expect("a stated patient");
    assert_eq!((p.age, p.sex.as_str()), (71, "male"));
    assert_eq!(sex_of(acs), Some(Sex::M), "male, as the pack spells it");
    assert_eq!(sex_of(by_id(&cases, "world-copd-woman")), Some(Sex::F));
    let rta = by_id(&cases, "world-rta-adult");
    assert!(rta.patient.is_none(), "no stated patient");
    assert_eq!(sex_of(rta), None, "so no sex to match");
    let dengue = by_id(&cases, "world-dengue-thailand");
    assert_eq!((dengue.country.as_deref(), dengue.endemic, dengue.provisional), (Some("THA"), true, true));
    // The real payload carries `derivations` beside `cases`; it is ignored. A bare array is the
    // same list; another host is not a list; the letters f/m are read as the pool spells them.
    assert_eq!(parse_cases(&serde_json::to_string(&cases).unwrap()).unwrap(), cases);
    assert!(parse_cases(r#"{"the_ward_is":"https://elsewhere"}"#).is_err());
    assert!(parse_cases(r#"{"derivations":{}}"#).is_err(), "no cases is not a list");
    let letters = parse_cases(r#"{"cases":[{"case_id":"x","difficulty":"intern","patient":{"age":30,"sex":"F"}}],"derivations":{}}"#).unwrap();
    assert_eq!(sex_of(&letters[0]), Some(Sex::F));
    assert_eq!(letters[0].archetype, None);
}

#[test]
fn the_age_window_is_twelve_years_either_side_and_children_stay_children() {
    let cases = cases();
    assert_eq!((AGE_SLACK, CHILD_UNDER), (12, 16));
    assert_eq!(age_window(by_id(&cases, "world-acs-elderly-man")), 59..=83);
    assert_eq!(age_window(by_id(&cases, "world-pneumothorax-young-man")), 16..=37, "an adult case never takes a child");
    assert_eq!(age_window(by_id(&cases, "world-appendicitis-teen")), 2..=15, "a case under sixteen takes only a persona under sixteen");
    assert_eq!(age_window(by_id(&cases, "world-febrile-toddler")), 1..=15, "and nobody is nought");
    assert_eq!(age_window(by_id(&cases, "world-rta-adult")), ADULT_ANY, "no stated patient: any adult");
    assert_eq!(ADULT_ANY, 18..=85);
}

#[test]
fn a_persona_fits_a_case_by_sex_and_by_the_window_and_a_case_with_no_patient_fits_any_adult() {
    let cases = cases();
    let acs = by_id(&cases, "world-acs-elderly-man");
    assert!(fits(acs, Sex::M, 63) && fits(acs, Sex::M, 59) && fits(acs, Sex::M, 83));
    assert!(!fits(acs, Sex::F, 63), "written for a man");
    assert!(!fits(acs, Sex::M, 58) && !fits(acs, Sex::M, 84));
    let toddler = by_id(&cases, "world-febrile-toddler");
    assert!(fits(toddler, Sex::F, 3) && fits(toddler, Sex::F, 15));
    assert!(!fits(toddler, Sex::F, 16), "a child's case never takes an adult");
    let rta = by_id(&cases, "world-rta-adult");
    assert!(fits(rta, Sex::F, 28) && fits(rta, Sex::M, 63) && fits(rta, Sex::M, 85));
    assert!(!fits(rta, Sex::M, 15) && !fits(rta, Sex::F, 86));
}

#[test]
fn the_cases_for_a_country_come_endemic_first_on_the_one_draw_in_five_then_the_common_draw_by_the_level_the_queue_is_short_of() {
    let cases = live();
    // Thailand has an endemic list — dengue is placeable and tagged for THA — and Indonesia none.
    assert!(has_endemic_list(&cases, "THA") && has_endemic_list(&cases, "KEN") && has_endemic_list(&cases, "NPL"));
    assert!(!has_endemic_list(&cases, "IDN") && !has_endemic_list(&cases, "IND"), "a case written for IND but not tagged endemic is not an endemic list");
    // On the endemic draw: dengue first, then every common case; never another country's case.
    let ranked = rank(&cases, "THA", &nobody(), &Mix::default(), 1, 0, true);
    assert_eq!(ranked[0].0.case_id, "world-dengue-thailand");
    assert!(ranked[0].1.contains("endemic"), "{}", ranked[0].1);
    let rest = ids(&ranked[1..]);
    assert_eq!(rest.len(), 14, "the fourteen common cases");
    for other in ["world-altitude-nepal", "world-malaria-kenya", "world-tb-community-india", "world-withdrawn-angina"] {
        assert!(!rest.contains(&other), "{other} is another country's, or withdrawn");
    }
    // Off the draw: the common cases only — dengue is not offered.
    let ranked = rank(&cases, "THA", &nobody(), &Mix::default(), 1, 0, false);
    assert_eq!(ids(&ranked).len(), 14);
    assert!(!ids(&ranked).contains(&"world-dengue-thailand"));
    // India: the community tuberculosis case is written for IND though not tagged endemic; it is
    // part of an Indian patient's common draw, on or off the endemic draw, and nobody else's.
    for draw in [true, false] {
        let ranked = rank(&cases, "IND", &nobody(), &Mix::default(), 1, 0, draw);
        assert!(ids(&ranked).contains(&"world-tb-community-india"), "{draw}: {:?}", ids(&ranked));
        assert_eq!(ids(&ranked).len(), 15);
    }
    let ranked = rank(&cases, "IDN", &nobody(), &Mix::default(), 1, 0, true);
    assert_eq!(ids(&ranked).len(), 14);
    assert!(!ids(&ranked).contains(&"world-tb-community-india"));
    // A case in a bed is not on the list at all.
    let beds: BTreeSet<String> = ["world-dengue-thailand".to_string(), "world-acs-elderly-man".to_string()].into_iter().collect();
    let ranked = rank(&cases, "THA", &beds, &Mix::default(), 1, 0, true);
    assert!(!ids(&ranked).contains(&"world-dengue-thailand") && !ids(&ranked).contains(&"world-acs-elderly-man"));
    assert_eq!(ids(&ranked).len(), 13);
    // A withdrawn row that arrives anyway is never chosen.
    let with_withdrawn = parse_cases(CASES).unwrap();
    let ranked = rank(&with_withdrawn, "IDN", &nobody(), &Mix::default(), 1, 0, true);
    assert!(!ids(&ranked).contains(&"world-withdrawn-angina"));
    // The draw itself: one in five per country, slot and seed, and only for a country with a
    // list — over a hundred slots THA draws endemic about a fifth of the time, IDN never.
    let tha = (0..100).filter(|slot| endemic_draw(&cases, "THA", 7, *slot)).count();
    assert!((10..=35).contains(&tha), "{tha} of 100, one in {ENDEMIC_IN} expected");
    assert_eq!((0..100).filter(|slot| endemic_draw(&cases, "IDN", 7, *slot)).count(), 0);
    // The level the board and queue are short of comes first: six residents waiting, no
    // students — every student case before every intern before every resident.
    let mut mix = Mix::default();
    for n in 0..6 {
        mix.count("resident", &format!("r{n}"));
    }
    for n in 0..3 {
        mix.count("intern", &format!("i{n}"));
    }
    let ranked = rank(&cases, "IDN", &nobody(), &mix, 1, 0, false);
    let levels: Vec<&str> = ranked.iter().map(|(c, _)| c.difficulty.as_str()).collect();
    let first_intern = levels.iter().position(|l| *l == "intern").unwrap();
    let first_resident = levels.iter().position(|l| *l == "resident").unwrap();
    assert!(levels[..first_intern].iter().all(|l| *l == "student"), "{levels:?}");
    assert!(levels[first_intern..first_resident].iter().all(|l| *l == "intern"), "{levels:?}");
    assert!(ranked[0].1.contains("student"), "{}", ranked[0].1);
    // Within a level, the case least often in the queue already comes first: students are the
    // level short (three to four and four), and among the three student cases the one never
    // queued leads, then the one queued once, then the one queued twice.
    let mut mix = Mix::default();
    mix.count("student", "world-acs-elderly-man");
    mix.count("student", "world-acs-elderly-man");
    mix.count("student", "world-copd-woman");
    for n in 0..4 {
        mix.count("intern", &format!("i{n}"));
        mix.count("resident", &format!("r{n}"));
    }
    let ranked = rank(&cases, "IDN", &nobody(), &mix, 1, 0, false);
    assert_eq!(ranked[0].0.case_id, "world-syncope-adult", "{:?}", ids(&ranked));
    assert_eq!(ranked[1].0.case_id, "world-copd-woman");
    assert_eq!(ranked[2].0.case_id, "world-acs-elderly-man");
    assert_eq!((mix.of("student"), mix.queued("world-acs-elderly-man"), mix.of("intern")), (3, 2, 4));
}

#[test]
fn the_ranking_is_the_same_for_the_same_seed_and_slot_and_the_seed_only_breaks_ties() {
    let cases = live();
    let a = rank(&cases, "IDN", &nobody(), &Mix::default(), 3, 0, false);
    let b = rank(&cases, "IDN", &nobody(), &Mix::default(), 3, 0, false);
    assert_eq!(ids(&a), ids(&b));
    let c = rank(&cases, "IDN", &nobody(), &Mix::default(), 4, 0, false);
    assert_ne!(ids(&a), ids(&c), "another seed, another order among the ties");
    assert_eq!(ids(&a).iter().collect::<BTreeSet<_>>(), ids(&c).iter().collect::<BTreeSet<_>>(), "the same cases, though");
    assert!(rank(&[], "IDN", &nobody(), &Mix::default(), 1, 0, true).is_empty(), "no list, no cases");
}

/// Staging on 17 Sep, revision 00034: every placeable case is endemic and written for a country
/// — the common draw is empty. Then the case written for home is the only case there is, and it
/// is taken on every draw, not one in five, so a ward with such a list still gets patients; a
/// country without a case gets none, and the plan says why.
#[test]
fn when_nothing_of_the_common_draw_is_placeable_the_case_written_for_home_is_taken_on_every_draw() {
    let only_endemic: Vec<WardCase> = live().into_iter().filter(|c| c.endemic).collect();
    assert_eq!(only_endemic.len(), 3);
    for draw in [true, false] {
        let ranked = rank(&only_endemic, "THA", &nobody(), &Mix::default(), 1, 0, draw);
        assert_eq!(ids(&ranked), vec!["world-dengue-thailand"], "{draw}");
        assert!(ranked[0].1.contains("endemic in THA"), "{}", ranked[0].1);
        if !draw {
            assert!(ranked[0].1.contains("only"), "outside the one-in-five, and said so: {}", ranked[0].1);
        }
        assert!(rank(&only_endemic, "IDN", &nobody(), &Mix::default(), 1, 0, draw).is_empty(), "no case is written for IDN and none is common");
    }
}
