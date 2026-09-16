//! Building a pack is a pure draw over four things the factory already holds: the ward's case
//! list, the pool, the portrait manifest and the ward's last answer (plus its own ledger of what
//! it sent). No network in any of these, and the same inputs with the same seed give the same
//! packs — a factory that built a different patient on each run from the same state would be one
//! nobody could reproduce, and reproducing it is how a stranger checks us.
//!
//! Since 16 Sep the case comes first and the person second: a pack's `case` is a World case_id
//! from `GET /api/ward/cases`, chosen for the country need drew, and the person is one of that
//! country of the case's sex, at an age inside the case's window. There is no season catalogue.

use std::collections::BTreeMap;
use vitals_factory::cases::{age_window, fits};
use vitals_factory::door::{parse_cases, BoardPatient, WardCase, WardView};
use vitals_factory::ledger::{Ledger, Sent};
use vitals_factory::manifest::{batch_age, Manifest};
use vitals_factory::need::{weights, Weights};
use vitals_factory::plan::{plan, Base, Inputs};
use vitals_factory::pool::{read_pool, Person};
use vitals_factory::sex::Sex;
use vitals_web::ward_chain::{is_portrait_url, pack_id, PORTRAITS};

const POOL: &str = include_str!("../../vitals-web/data/personas.json");
const PHYSICIANS: &str = include_str!("../../vitals-web/data/physicians.json");
const CASES: &str = include_str!("fixtures/ward-cases-2026-09-16.json");

/// The ward's list as the fixture has it: eighteen World cases, both sexes at every level, two
/// children's cases, two with no stated patient, four written for a country.
fn cases() -> Vec<WardCase> {
    parse_cases(CASES).expect("the fixture parses")
}

/// Only these cases of the list.
fn only(ids: &[&str]) -> Vec<WardCase> {
    cases().into_iter().filter(|c| ids.contains(&c.case_id.as_str())).collect()
}

fn by_id<'a>(cases: &'a [WardCase], id: &str) -> &'a WardCase {
    cases.iter().find(|c| c.case_id == id).unwrap_or_else(|| panic!("{id}"))
}

fn url(tag: &str) -> String {
    let mut h = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut h, tag.as_bytes());
    let sha: String = sha2::Digest::finalize(h).iter().map(|b| format!("{b:02x}")).collect();
    format!("{PORTRAITS}/{sha}.webp")
}

/// Every persona has a `stable` face, made at the batch ages (28/45/63 by index), as the seeded
/// manifest is.
fn full_manifest(pool: &[Person]) -> Manifest {
    let mut m = Manifest::default();
    for p in pool {
        m.record_base(&p.key, batch_age(&p.key).unwrap(), &url(&p.key), p);
    }
    m
}

/// Equal need everywhere, so the tests of the other rules are not moved by the weighting.
fn flat(pool: &[Person]) -> Weights {
    Weights::flat(pool)
}

fn empty_ward() -> WardView {
    WardView { readable: true, source: "test".into(), why: None, beds: 3, catalogue: vec![], queue: None, patients: vec![] }
}

fn on_board(id: u64, state: &str, p: &Person, case: &str, age: u16) -> BoardPatient {
    BoardPatient {
        patient_id: id, state: state.into(), bed: Some(1), name: Some(p.name.clone()), age: Some(age),
        country: Some(p.country.clone()), case: Some(case.into()), endemic: false, portrait: None, portraits: BTreeMap::new(),
    }
}

fn person<'a>(pool: &'a [Person], key: &str) -> &'a Person {
    pool.iter().find(|p| p.key == key).expect(key)
}

#[test]
fn the_pool_reads_from_the_wards_own_file() {
    let pool = read_pool(POOL).expect("the pool parses");
    assert_eq!(pool.len(), 400, "sixty at first, deeper where the need is, and the whole world since 16 Sep");
    let ploy = person(&pool, "THA-0");
    assert_eq!((ploy.name.as_str(), ploy.sex, ploy.country.as_str(), ploy.place.as_str()), ("Ploy Siriwattana", Sex::F, "THA", "Thailand"));
    assert_eq!(person(&pool, "IDN-2").name, "Agus Pratama");
}

#[test]
fn the_same_inputs_and_seed_give_the_same_packs() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, ward, ledger) = (cases(), full_manifest(&pool), empty_ward(), Ledger::default());
    let i = Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &ward, ledger: &ledger, weights: &flat(&pool), beds: 3, want: 6, seed: 7 };
    let a = plan(&i);
    let b = plan(&i);
    assert_eq!(a.packs.len(), 6);
    assert_eq!(a.packs, b.packs);
    let c = plan(&Inputs { seed: 8, ..i });
    assert_ne!(a.packs, c.packs, "another seed is another draw");
}

/// The door of 0543ed7 takes a World case_id, a person of the case's sex at an age inside its
/// window, a real country, portraits at the bucket's address — and refuses a season id. Every
/// pack the plan builds is one it would take, and says which case it is for and at what level.
#[test]
fn every_pack_is_one_the_door_would_take_and_agrees_with_its_own_case() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, ward, ledger) = (cases(), full_manifest(&pool), empty_ward(), Ledger::default());
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &ward, ledger: &ledger, weights: &flat(&pool), beds: 3, want: 20, seed: 1 });
    assert_eq!(p.packs.len(), 20, "{:?}", p.notes);
    for pl in &p.packs {
        let case = by_id(&cat, &pl.pack.case);
        assert!(!pl.pack.case.starts_with("osce-") && !pl.pack.case.starts_with("ep"), "{}: a season id", pl.pack.case);
        let who = person(&pool, &pl.person);
        assert!(fits(case, who.sex, pl.pack.persona.age), "{}: {} {} does not fit {:?} {:?}", case.case_id, who.sex.word(), pl.pack.persona.age, case.patient, age_window(case));
        assert!(case.country.is_none() || case.country.as_deref() == Some(who.country.as_str()), "{}: another country's case on {}", case.case_id, who.name);
        assert_eq!(pl.pack.endemic, case.endemic && case.country.as_deref() == Some(who.country.as_str()), "{}: the tag is the case's, for her country", case.case_id);
        assert_eq!(pl.level, case.difficulty, "{}: the level is the list's", case.case_id);
        assert!(!pl.case_why.is_empty());
        assert_eq!(pl.pack.persona.name, who.name);
        assert_eq!(pl.pack.persona.country, who.country);
        assert_eq!(pl.pack.persona.sex, who.sex.letter());
        assert_eq!(pl.sex, who.sex);
        assert!(pl.pack.portrait.values().all(|u| is_portrait_url(u)), "{:?}", pl.pack.portrait);
    }
}

#[test]
fn nobody_is_on_the_ward_twice() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man) = (cases(), full_manifest(&pool));
    let ploy = person(&pool, "THA-0");
    let anan = person(&pool, "THA-1");
    let mut ward = empty_ward();
    ward.patients.push(on_board(1, "on_shift", ploy, "osce-c2", 50));
    ward.patients.push(on_board(2, "went_home", person(&pool, "IDN-1"), "osce-b", 25));
    let mut ledger = Ledger::default();
    ledger.sent.insert("deadbeef".into(), Sent::new("osce-a", anan, 70, false, None, 1, "test"));
    // Two hundred wanted from a pool of three hundred and more (sixty beds, so the bed cap plays
    // no part here): nobody is drawn twice and the two who are busy are not drawn at all.
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &ward, ledger: &ledger, weights: &flat(&pool), beds: 60, want: 200, seed: 3 });
    let keys: Vec<&str> = p.packs.iter().map(|pl| pl.person.as_str()).collect();
    assert_eq!(keys.len(), 200, "{:?}", p.notes);
    assert!(!keys.contains(&"THA-0"), "Ploy is in a bed");
    assert!(!keys.contains(&"THA-1"), "Anan is queued and unseen");
    let mut dedup = keys.clone();
    dedup.sort_unstable();
    dedup.dedup();
    assert_eq!(dedup.len(), keys.len(), "no face twice in one plan");
    // Budi went home, so his face is free again: with everyone else busy he is the one drawn.
    let mut all_but_budi = Ledger::default();
    for (n, x) in pool.iter().enumerate().filter(|(_, x)| x.key != "IDN-1") {
        all_but_budi.sent.insert(format!("id{n}"), Sent::new("osce-a", x, 70, false, None, 1, "test"));
    }
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &ward, ledger: &all_but_budi, weights: &flat(&pool), beds: 60, want: 3, seed: 3 });
    assert_eq!(p.packs.iter().map(|pl| pl.person.as_str()).collect::<Vec<_>>(), vec!["IDN-1"]);
}

/// Student, intern and resident about 1:1:1 across the board and the queue (the founder's rule:
/// levels exist so a stranger can choose), and no case twice on the board.
#[test]
fn the_levels_are_balanced_against_what_the_ward_already_holds_and_no_case_sits_in_two_beds() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man) = (cases(), full_manifest(&pool));
    let count = |p: &vitals_factory::plan::Plan, level: &str| p.packs.iter().filter(|pl| pl.level == level).count();

    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&pool), beds: 3, want: 9, seed: 2 });
    assert_eq!((count(&p, "student"), count(&p, "intern"), count(&p, "resident")), (3, 3, 3), "{:?}", p.packs.iter().map(|x| x.pack.case.clone()).collect::<Vec<_>>());

    // Two interns already in beds: the next two go to the other levels before intern gets
    // another; and the two cases in beds are chosen for nobody.
    let mut ward = empty_ward();
    ward.patients.push(on_board(1, "on_ward", person(&pool, "JPN-1"), "world-pneumothorax-young-man", 25));
    ward.patients.push(on_board(2, "on_ward", person(&pool, "KOR-0"), "world-cholecystitis-woman", 50));
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &ward, ledger: &Ledger::default(), weights: &flat(&pool), beds: 3, want: 2, seed: 2 });
    assert_eq!(count(&p, "intern"), 0, "{:?}", p.packs.iter().map(|x| x.pack.case.clone()).collect::<Vec<_>>());
    assert_eq!(count(&p, "student") + count(&p, "resident"), 2);
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &ward, ledger: &Ledger::default(), weights: &flat(&pool), beds: 60, want: 40, seed: 2 });
    assert!(p.packs.iter().all(|pl| pl.pack.case != "world-pneumothorax-young-man" && pl.pack.case != "world-cholecystitis-woman"), "{:?}", p.packs.iter().map(|x| x.pack.case.clone()).collect::<Vec<_>>());
    // The queue's levels count too: eight residents waiting, the next nine hold no resident.
    let mut ledger = Ledger::default();
    for (n, who) in pool.iter().filter(|x| x.sex == Sex::F).take(8).enumerate() {
        let mut s = Sent::new("world-sepsis-woman", who, 55, false, None, 1, "test");
        s.difficulty = Some("resident".into());
        ledger.sent.insert(format!("r{n}"), s);
    }
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &ledger, weights: &flat(&pool), beds: 3, want: 9, seed: 2 });
    assert_eq!(count(&p, "resident"), 0, "{:?}", p.packs.iter().map(|x| x.pack.case.clone()).collect::<Vec<_>>());
}

/// Her age is the persona's, drawn to fit the case: a face that already fits the case's window
/// is used and the age stays near it; when none fits, one is made at an age inside the window.
#[test]
fn a_face_already_made_is_used_and_a_missing_one_is_made_at_her_age() {
    let pool = read_pool(POOL).unwrap();
    // A man of 71 (59–83) and a woman of 68 (56–80): the batch's 63-year-old faces fit both, so
    // every pack carries a stable portrait, needs nothing made, and stays near the face's age.
    let elders = only(&["world-acs-elderly-man", "world-copd-woman"]);
    let man = full_manifest(&pool);
    let p = plan(&Inputs { cases: &elders, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&pool), beds: 3, want: 5, seed: 4 });
    assert_eq!(p.packs.len(), 5, "{:?}", p.notes);
    for pl in &p.packs {
        let Base::Have { url, age, .. } = &pl.base else { panic!("{}: a face exists at 63 and fits", pl.person) };
        assert_eq!(pl.pack.portrait.get("stable"), Some(url));
        assert!(*age == 63, "the face that fits is the 63-year-old's");
        assert!((pl.pack.persona.age as i32 - 63).abs() <= 3, "her age stays near the face's: {}", pl.pack.persona.age);
        assert!(age_window(by_id(&elders, &pl.pack.case)).contains(&pl.pack.persona.age));
        let idx: usize = pl.person.rsplit('-').next().unwrap().parse().unwrap();
        assert_eq!(idx % 3, 2, "every third person's face is the 63-year-old's in this manifest: {}", pl.person);
    }
    // A case with no stated patient fits any adult: every batch face fits, nothing is made.
    let any = only(&["world-rta-adult"]);
    let p = plan(&Inputs { cases: &any, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&pool), beds: 3, want: 6, seed: 4 });
    assert_eq!(p.packs.len(), 6);
    assert!(p.packs.iter().all(|pl| matches!(pl.base, Base::Have { .. })), "{:?}", p.packs.iter().map(|x| x.base.clone()).collect::<Vec<_>>());
    assert!(p.packs.iter().all(|pl| (18..=85).contains(&pl.pack.persona.age)));
    // A child's case: no adult face fits, so one is to be made at the age drawn inside the
    // window, and the pack goes out without a picture rather than with a wrong one.
    let child = only(&["world-asthma-child"]);
    let p = plan(&Inputs { cases: &child, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&pool), beds: 3, want: 2, seed: 4 });
    assert_eq!(p.packs.len(), 2);
    for pl in &p.packs {
        let Base::Make { key, age } = &pl.base else { panic!("no adult face fits a child") };
        assert_eq!(key, &pl.person);
        assert!((1..=15).contains(age) && *age == pl.pack.persona.age, "{age}");
        assert_eq!(pl.sex, Sex::F, "written for a girl");
        assert!(pl.pack.portrait.is_empty(), "a missing picture is never a reason to withhold a patient, and never a wrong picture");
    }
    // Once that face is recorded under key@age, the next plan uses it.
    let mut man2 = man.clone();
    let first = &p.packs[0];
    man2.record_base(&first.person, first.pack.persona.age, &url("child"), person(&pool, &first.person));
    let p2 = plan(&Inputs { cases: &child, pool: &pool, manifest: &man2, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&pool), beds: 3, want: 2, seed: 4 });
    let again = p2.packs.iter().find(|pl| pl.person == first.person).expect("same seed, same person");
    assert!(matches!(&again.base, Base::Have { url: u, .. } if u == &url("child")));
}

#[test]
fn pool_exhausted_builds_nothing_and_says_so() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man) = (cases(), full_manifest(&pool));
    let mut ledger = Ledger::default();
    for (n, p) in pool.iter().enumerate() {
        ledger.sent.insert(format!("id{n}"), Sent::new("world-rta-adult", p, 70, false, None, 1, "test"));
    }
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &ledger, weights: &flat(&pool), beds: 3, want: 3, seed: 1 });
    assert!(p.packs.is_empty());
    assert!(p.exhausted);
    assert!(p.notes.iter().any(|n| n.contains("pool exhausted")), "{:?}", p.notes);
}

/// No cases: nothing is built and the plan says so — never a season id, never an empty case.
#[test]
fn a_ward_with_no_cases_gets_nothing_built_and_the_plan_says_so() {
    let pool = read_pool(POOL).unwrap();
    let man = full_manifest(&pool);
    let p = plan(&Inputs { cases: &[], pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&pool), beds: 3, want: 3, seed: 1 });
    assert!(p.packs.is_empty() && p.exhausted);
    assert!(p.notes.iter().any(|n| n.contains("no cases")), "{:?}", p.notes);
}

/// Case first, then a person of the case's sex: with only men's cases on the list, only men are
/// drawn, and a country whose free people are all women is passed over, never forced.
#[test]
fn a_persona_whose_sex_no_case_was_written_for_is_skipped_not_forced() {
    let pool = read_pool(POOL).unwrap();
    let man = full_manifest(&pool);
    let men_only = only(&["world-acs-elderly-man", "world-stroke-man", "world-pneumothorax-young-man"]);
    let p = plan(&Inputs { cases: &men_only, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&pool), beds: 3, want: 400, seed: 1 });
    let men = pool.iter().filter(|x| x.sex == Sex::M).count();
    assert!(p.packs.iter().all(|pl| pl.sex == Sex::M), "no woman on a case written for a man");
    assert!(!p.packs.is_empty() && p.packs.len() <= men, "{} packs of {men} men", p.packs.len());
    assert!(p.exhausted, "and then the pool is exhausted for this list");
    // Viet Nam has two women and one man free: a men's case takes the man, and when he is busy
    // the country is skipped for that case rather than a woman forced onto it.
    let vietnam: Vec<Person> = pool.iter().filter(|x| x.country == "VNM").cloned().collect();
    let p = plan(&Inputs { cases: &men_only, pool: &vietnam, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&vietnam), beds: 3, want: 3, seed: 1 });
    assert_eq!(p.packs.iter().map(|pl| pl.person.as_str()).collect::<Vec<_>>(), vec!["VNM-1"]);
    assert!(p.exhausted);
}

/// The endemic tag is the case's, for her country, from the ward's own list: a Thai woman in the
/// dengue window gets dengue and the tag; a Kenyan man in the malaria window gets malaria; nobody
/// else gets either, and a case with no country never carries the tag.
#[test]
fn endemic_is_true_only_when_the_list_pairs_her_country_with_her_case() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man) = (cases(), full_manifest(&pool));
    let mut endemic_seen = 0;
    for seed in 0..40 {
        let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&pool), beds: 3, want: 12, seed });
        for pl in &p.packs {
            let case = by_id(&cat, &pl.pack.case);
            if pl.pack.endemic {
                endemic_seen += 1;
                assert!(case.endemic && case.country.as_deref() == Some(pl.pack.persona.country.as_str()), "{:?}", pl.pack);
            } else {
                assert!(!(case.endemic && case.country.as_deref() == Some(pl.pack.persona.country.as_str())), "{:?} should carry the tag", pl.pack);
            }
        }
    }
    assert!(endemic_seen > 0, "over forty seeds a Thai woman or a Kenyan man is drawn and gets the case written for home");
    // Thailand alone, six packs: the first Thai woman drawn gets dengue, at an age inside 16–38,
    // tagged; once dengue is waiting the next Thai women take the common draw — the case written
    // for home comes first while it is not already on the ward or in the queue, so a queue of six
    // Thais is not six dengues.
    let thai: Vec<Person> = pool.iter().filter(|x| x.country == "THA").cloned().collect();
    let p = plan(&Inputs { cases: &cat, pool: &thai, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&thai), beds: 3, want: 6, seed: 5 });
    let dengue: Vec<_> = p.packs.iter().filter(|pl| pl.pack.case == "world-dengue-thailand").collect();
    assert_eq!(dengue.len(), 1, "{:?}", p.packs.iter().map(|x| x.pack.case.clone()).collect::<Vec<_>>());
    assert_eq!(dengue[0].pack.case, p.packs[0].pack.case, "and it is the first");
    assert!(dengue[0].pack.endemic && dengue[0].sex == Sex::F && (16..=38).contains(&dengue[0].pack.persona.age), "{:?}", dengue[0].pack);
    // Nepal's altitude case has no stated patient: any Nepali adult, tagged.
    let nepal: Vec<Person> = pool.iter().filter(|x| x.country == "NPL").cloned().collect();
    let p = plan(&Inputs { cases: &cat, pool: &nepal, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &flat(&nepal), beds: 3, want: 1, seed: 5 });
    assert_eq!(p.packs[0].pack.case, "world-altitude-nepal");
    assert!(p.packs[0].pack.endemic && (18..=85).contains(&p.packs[0].pack.persona.age));
}

#[test]
fn the_manifest_reads_the_seeded_shape_and_the_flat_one_and_knows_the_batch_ages() {
    let seeded = r#"{"THA-0": {"name": "Ploy Siriwattana", "country": "THA", "sex": "f", "portrait": {"stable": "https://storage.googleapis.com/vitals-world-portraits/a.webp", "critical": "https://storage.googleapis.com/vitals-world-portraits/b.webp"}},
                     "THA-1": {"stable": "https://storage.googleapis.com/vitals-world-portraits/c.webp"},
                     "THA-1@71": {"age": 71, "portrait": {"stable": "https://storage.googleapis.com/vitals-world-portraits/d.webp"}}}"#;
    let m = Manifest::parse(seeded).expect("both shapes parse");
    assert_eq!(m.entries.len(), 3);
    assert_eq!(m.entries["THA-1"].portrait["stable"], "https://storage.googleapis.com/vitals-world-portraits/c.webp");
    assert_eq!(m.entries["THA-0"].portrait.len(), 2);
    assert_eq!(batch_age("THA-0"), Some(28));
    assert_eq!(batch_age("THA-1"), Some(45));
    assert_eq!(batch_age("BRA-2"), Some(63));
    assert_eq!(batch_age("THA-1@71"), None, "an age-keyed face carries its own age");
    // A seeded face with no age recorded is the batch's, and fits only a band that holds that age.
    let b = m.base_for("THA-1", &(40..=50)).expect("45 fits");
    assert_eq!((b.age, b.url.as_str()), (45, "https://storage.googleapis.com/vitals-world-portraits/c.webp"));
    let b = m.base_for("THA-1", &(65..=80)).expect("the 71 face fits");
    assert_eq!((b.age, b.url.as_str()), (71, "https://storage.googleapis.com/vitals-world-portraits/d.webp"));
    assert!(m.base_for("THA-1", &(5..=7)).is_none());
    assert!(m.base_for("THA-0", &(20..=30)).is_some());
    // Round trip keeps every entry.
    let again = Manifest::parse(&m.to_json()).unwrap();
    assert_eq!(again, m);
}

#[test]
fn the_ledger_learns_from_the_board_who_was_admitted_and_who_has_left() {
    let pool = read_pool(POOL).unwrap();
    let ploy = person(&pool, "THA-0");
    let mut ledger = Ledger::default();
    let sent = Sent::new("osce-a2", ploy, 66, false, Some(url("ploy")), 100, "https://ward");
    let id = pack_id(&sent.to_pack());
    ledger.sent.insert(id.clone(), sent);
    assert_eq!(ledger.unseen().len(), 1);

    let mut ward = empty_ward();
    ward.patients.push(on_board(4242, "on_ward", ploy, "osce-a2", 66));
    let notes = ledger.reconcile(&ward);
    assert_eq!(ledger.sent[&id].patient_id, Some(4242));
    assert!(ledger.unseen().is_empty());
    assert!(notes.iter().any(|n| n.contains("4242")), "{notes:?}");
    assert!(ledger.busy_keys(&ward, &pool).contains("THA-0"), "she is in a bed");

    ward.patients[0].state = "went_home".into();
    ledger.reconcile(&ward);
    assert!(ledger.sent[&id].closed);
    assert!(!ledger.busy_keys(&ward, &pool).contains("THA-0"), "she went home; the face is free");

    // A sentence about a patient takes the patient's pronoun from the pack's sex: a man who has
    // left is "his face is free", never "her".
    let budi = person(&pool, "IDN-1");
    let sent_m = Sent::new("osce-b", budi, 25, false, None, 100, "https://ward");
    let id_m = pack_id(&sent_m.to_pack());
    ledger.sent.insert(id_m.clone(), sent_m);
    ward.patients.push(on_board(5150, "died", budi, "osce-b", 25));
    let notes = ledger.reconcile(&ward);
    assert!(notes.iter().any(|n| n.contains("Budi Santoso (5150) has left — died; his face is free")), "{notes:?}");
    assert!(notes.iter().all(|n| !n.contains("her face") || n.contains("Ploy")), "{notes:?}");

    // Somebody on the board this ledger never sent (another factory, a hand push) is busy too.
    ward.patients.push(on_board(7, "on_shift", person(&pool, "NGA-2"), "osce-d4", 70));
    assert!(ledger.busy_keys(&ward, &pool).contains("NGA-2"));
    let round = Ledger::parse(&ledger.to_json()).unwrap();
    assert_eq!(round, ledger);
}

// ── need ─────────────────────────────────────────────────────────────────────
// Founder, 16 Sep 10:40: the ward's patients must reflect real need. The country draw is weighted
// by people per doctor (World Bank SH.MED.PHYS.ZS, latest value, 1000 / per_1000 to the nearest
// ten), floored at the pooled countries' median ÷ 4 so every country still appears, and a country
// with no value gets the median.

#[test]
fn weights_are_people_per_doctor_with_a_floor_and_the_median_for_the_unknown() {
    let pool = read_pool(POOL).unwrap();
    let w = weights(PHYSICIANS, &pool).expect("the file parses");
    assert_eq!(w.median, 1110.0, "the median of the seventy-four, as the data stands on 16 Sep 2026");
    assert_eq!(w.floor, 277.5);
    assert_eq!(w.of("NER"), 26320.0, "Niger, 1:26,320 (2023) — the worst shortage in the pool, and the largest weight");
    assert_eq!(w.of("ETH"), 6990.0, "Ethiopia, 1:6,990 (2023)");
    assert_eq!(w.of("JPN"), 380.0);
    assert_eq!(w.of("USA"), 277.5, "the United States (270) is lifted to the floor");
    assert_eq!(w.of("GRC"), 277.5, "Greece (150) likewise");
    assert_eq!(w.year("NER"), Some(2023));
    let ranked_owned = w.ranked();
    let ranked: Vec<&str> = ranked_owned.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(&ranked[..5], &["NER", "SSD", "SOM", "MWI", "PNG"], "the five highest by the numbers, not by a list");
    assert_eq!(&ranked[5..10], &["BDI", "TCD", "RWA", "YEM", "SEN"]);
    assert_eq!(ranked.last().copied(), Some("USA"), "eleven countries share the floor; ties go by code");
    assert!((w.of("ETH") / w.of("JPN") - 18.4).abs() < 0.1, "Ethiopia about eighteen times Japan");
    assert!((w.of("NER") / w.of("ETH") - 3.77).abs() < 0.01, "Niger nearly four times Ethiopia");
    // A pooled country the series has no value for gets the median.
    let mut with_unknown = pool.clone();
    with_unknown.push(Person { key: "ATA-0".into(), name: "Nobody Here".into(), sex: Sex::F, country: "ATA".into(), place: "Antarctica".into() });
    let w2 = weights(PHYSICIANS, &with_unknown).unwrap();
    assert_eq!(w2.of("ATA"), w2.median);
    assert!(w2.table().contains("NER 26,320") && w2.table().contains("floor 278"), "{}", w2.table());
}

/// Twice the need: twice the patients. Exact over a run, not merely likely — eight countries at
/// 2,000 and eight at 1,000, twenty-four packs: two each and one each, with the spread's cap of
/// two in twenty never in the way.
#[test]
fn the_draw_is_weighted_by_need_and_least_on_the_ward_breaks_ties() {
    let mut pool = Vec::new();
    let mut table = Vec::new();
    for n in 0..16 {
        let c = format!("{}{}{}", (b'A' + n) as char, (b'A' + n) as char, (b'A' + n) as char);
        table.push((c.clone(), if n < 8 { 2000.0 } else { 1000.0 }));
        for i in 0..6 {
            pool.push(Person { key: format!("{c}-{i}"), name: format!("Person {c} {i}"), sex: if i % 2 == 0 { Sex::F } else { Sex::M }, country: c.clone(), place: c.clone() });
        }
    }
    let cat = cases();
    let (man, ledger) = (Manifest::default(), Ledger::default());
    let rows: Vec<(&str, f64)> = table.iter().map(|(c, w)| (c.as_str(), *w)).collect();
    let w = Weights::from_table(&rows);
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &ledger, weights: &w, beds: 3, want: 24, seed: 5 });
    let seq: Vec<&str> = p.packs.iter().map(|x| x.pack.persona.country.as_str()).collect();
    assert_eq!(seq.len(), 24, "{:?}", p.notes);
    for (c, weight) in &table {
        let got = seq.iter().filter(|x| *x == c).count();
        assert_eq!(got, if *weight == 2000.0 { 2 } else { 1 }, "{c} at {weight}: {seq:?}");
    }
    assert!(p.packs.iter().any(|pl| pl.weight == 2000.0), "each pack carries the weight it was drawn with");

    // Equal need: whoever has fewer on the ward is drawn first. Only first — need is measured
    // against the run this factory has sent, not against the board, so once BBB has a pack the
    // two are no longer tied and AAA is behind; the board breaks ties, it does not count as need.
    let two: Vec<Person> = pool.iter().filter(|x| x.country == "AAA" || x.country == "BBB").cloned().collect();
    let w = Weights::from_table(&[("AAA", 1000.0), ("BBB", 1000.0)]);
    let mut ward = empty_ward();
    ward.patients.push(on_board(1, "on_ward", &two[0], "world-cholecystitis-woman", 50));
    ward.patients.push(on_board(2, "on_ward", &two[2], "world-copd-woman", 66));
    let p = plan(&Inputs { cases: &cat, pool: &two, manifest: &man, ward: &ward, ledger: &ledger, weights: &w, beds: 10, want: 2, seed: 5 });
    assert_eq!(p.packs.iter().map(|x| x.pack.persona.country.as_str()).collect::<Vec<_>>(), vec!["BBB", "AAA"]);
}

/// No country holds more than 40 % of the beds at once: with three beds that is one, so a
/// country with somebody in a bed is not drawn until she leaves.
#[test]
fn no_country_takes_more_than_its_share_of_the_beds() {
    let pool = read_pool(POOL).unwrap();
    let cat = cases();
    let (man, ledger) = (full_manifest(&pool), Ledger::default());
    let w = weights(PHYSICIANS, &pool).unwrap();
    let ranked: Vec<String> = w.ranked().into_iter().map(|(c, _)| c).collect();
    let (top, next) = (ranked[0].as_str(), ranked[1].as_str());
    let mut ward = empty_ward();
    ward.patients.push(on_board(1, "on_shift", person(&pool, &format!("{top}-0")), "world-copd-woman", 66));
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &ward, ledger: &ledger, weights: &w, beds: 3, want: 6, seed: 9 });
    assert_eq!(p.packs.len(), 6);
    assert!(p.packs.iter().all(|pl| pl.pack.persona.country != top), "{top} has the bed: {:?}", p.packs.iter().map(|x| x.pack.persona.country.clone()).collect::<Vec<_>>());
    assert_eq!(p.packs[0].pack.persona.country, next, "so the highest need without a bed goes first");
    // With ten beds the cap is four, so the bed does not keep {top} out; and a patient the ledger
    // never sent is not part of the run need is measured against, so {top} is first again.
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &ward, ledger: &ledger, weights: &w, beds: 10, want: 6, seed: 9 });
    assert_eq!(p.packs[0].pack.persona.country, top, "{:?}", p.packs.iter().map(|x| x.pack.persona.country.clone()).collect::<Vec<_>>());
    assert_eq!(p.packs.iter().filter(|pl| pl.pack.persona.country == top).count(), 1, "and once in six, near the share of the largest need");
}

/// The pool grows where the need is: among the twenty of the first spread, nine people for the
/// five highest-weighted countries, six for the next five, three for the rest — computed from the
/// weights over those twenty, not from a list — and four to six for each of the forty added on
/// 16 Sep (tests/world.rs holds those), with both sexes everywhere, full invented names, and no
/// name twice.
#[test]
fn the_pool_is_deeper_where_the_need_is() {
    let pool = read_pool(POOL).unwrap();
    let first_spread: Vec<Person> = pool.iter().take(105).cloned().collect();
    assert_eq!(first_spread.iter().map(|p| p.country.as_str()).collect::<std::collections::BTreeSet<_>>().len(), 20, "the twenty come first in the file");
    let w = weights(PHYSICIANS, &first_spread).unwrap();
    let ranked = w.ranked();
    let mut per_country: BTreeMap<&str, Vec<&Person>> = BTreeMap::new();
    for p in &pool {
        per_country.entry(p.country.as_str()).or_default().push(p);
    }
    for (rank, (country, _)) in ranked.iter().enumerate() {
        let want = if rank < 5 { 9 } else if rank < 10 { 6 } else { 3 };
        let people = &per_country[country.as_str()];
        assert_eq!(people.len(), want, "{country} is ranked {} among the twenty and should carry {want}", rank + 1);
    }
    for (country, people) in &per_country {
        assert!(people.iter().any(|p| p.sex == Sex::F) && people.iter().any(|p| p.sex == Sex::M), "{country}: both sexes");
        for p in people {
            assert!(p.name.contains(' '), "{}: a chart carries a full name", p.name);
        }
    }
    let mut names: Vec<&str> = pool.iter().map(|p| p.name.as_str()).collect();
    let n = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), n, "no name twice in the pool");
    assert_eq!(pool.len(), 5 * 9 + 5 * 6 + 10 * 3 + 295, "one hundred and five of the first spread, two hundred and ninety-five added");
    // The first three of every country are the sixty the faces were made for, in their order:
    // the manifest keys on the position in the file.
    assert_eq!(person(&pool, "THA-0").name, "Ploy Siriwattana");
    assert_eq!(person(&pool, "ETH-2").name, "Hanan Mohammed");
    assert_eq!(person(&pool, "USA-2").name, "Emily Novak");
}

/// The draw against the World Bank weights themselves, not a table written for the test: over
/// twenty packs from an empty ward each country lands within one of its share of twenty, Ethiopia
/// is drawn first and most, the floored United States at most once. This is the test that pins
/// plan() to `policy.where_they_come_from`: the country is drawn by need, and "least on the ward"
/// only breaks ties.
#[test]
fn the_draw_follows_the_world_bank_weights_from_the_file() {
    let pool = read_pool(POOL).unwrap();
    let cat = cases();
    let (man, ledger) = (full_manifest(&pool), Ledger::default());
    let w = weights(PHYSICIANS, &pool).unwrap();
    let total: f64 = w.by_country.values().sum();
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &ledger, weights: &w, beds: 3, want: 20, seed: 16 });
    assert_eq!(p.packs.len(), 20);
    let top = w.ranked()[0].0.clone();
    assert_eq!(p.packs[0].pack.persona.country, top, "the greatest need is drawn first");
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for pl in &p.packs {
        *count.entry(pl.pack.persona.country.as_str()).or_default() += 1;
        assert_eq!(pl.weight, w.of(&pl.pack.persona.country), "each pack carries the weight it was drawn with");
    }
    for (c, wc) in &w.by_country {
        let expected = (wc / total * 20.0).round() as isize;
        let got = count.get(c.as_str()).copied().unwrap_or(0) as isize;
        assert!((got - expected).abs() <= 1, "{c}: drawn {got}, share of twenty is {expected} (weight {wc})");
    }
    assert_eq!(count[top.as_str()], 2, "{top}, nine per cent of the need of seventy-four countries, two of twenty — and the cap");
    assert!(count.get("USA").copied().unwrap_or(0) <= 1, "the United States, at most once in twenty");
    assert!(count[top.as_str()] >= count.get("SSD").copied().unwrap_or(0), "{count:?}");
}

/// No sentence in the crate about a patient carries a fixed pronoun: the pack's sex chooses it,
/// or the sentence says "the face" / "the stable". prompts.rs builds pronouns and is the one
/// file allowed the words; the rest of the crate is held to it here, the way a grep would.
#[test]
fn no_sentence_about_a_patient_carries_a_fixed_pronoun() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let re_literal = |line: &str| -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = line;
        while let Some(i) = rest.find('"') {
            let after = &rest[i + 1..];
            let Some(j) = after.find('"') else { break };
            out.push(after[..j].to_string());
            rest = &after[j + 1..];
        }
        out
    };
    // A sentence: two or more words with a fixed pronoun among them. A literal that is only the
    // pronoun itself is the helper that chooses it (catalogue::Sex::possessive), not a sentence.
    let pronoun = |lit: &str| {
        let words: Vec<&str> = lit.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).collect();
        words.len() >= 2 && words.iter().any(|w| matches!(*w, "her" | "she" | "hers" | "his" | "him"))
    };
    let mut hits = Vec::new();
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name == "prompts.rs" {
            continue;
        }
        for (n, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for lit in re_literal(line) {
                if pronoun(&lit) {
                    hits.push(format!("{name}:{}: {lit}", n + 1));
                }
            }
        }
    }
    assert!(hits.is_empty(), "sentences with a fixed pronoun about a patient:\n{}", hits.join("\n"));
}
