//! Building a pack is a pure draw over four things the factory already holds: the catalogue, the
//! pool, the portrait manifest and the ward's last answer (plus its own ledger of what it sent).
//! No network in any of these, and the same inputs with the same seed give the same packs — a
//! factory that built a different patient on each run from the same state would be one nobody
//! could reproduce, and reproducing it is how a stranger checks us.

use std::collections::BTreeMap;
use vitals_factory::catalogue::{Case, Catalogue, Sex};
use vitals_factory::door::{BoardPatient, WardView};
use vitals_factory::ledger::{Ledger, Sent};
use vitals_factory::manifest::{batch_age, Manifest};
use vitals_factory::plan::{plan, Base, Inputs};
use vitals_factory::pool::{read_endemic, read_pool, Person};
use vitals_web::ward::{age_band, case_patient, difficulty_of};
use vitals_web::ward_chain::{pack_id, validate_pack, PORTRAITS};

const POOL: &str = include_str!("../../vitals-web/data/personas.json");
const ENDEMIC: &str = include_str!("../../vitals-web/data/endemic.json");

/// A station exactly as the door reads it — the only band a pack for it may carry.
fn case(id: &str) -> Case {
    let theirs = case_patient(id).expect("a station");
    Case {
        id: id.into(),
        sex: Sex::parse(&theirs.sex).unwrap(),
        band: age_band(theirs.age),
        difficulty: difficulty_of(id).expect("a ward case"),
        source: "test".into(),
    }
}

/// Every band, both sexes: osce-a M 71 and osce-a2 F 68 (student); osce-b M 25 and osce-c2 F 53
/// (intern); osce-c F 6, osce-d2 F 55 and osce-d4 F 72 (resident).
fn catalogue() -> Catalogue {
    Catalogue {
        cases: ["osce-a", "osce-a2", "osce-b", "osce-c2", "osce-c", "osce-d4", "osce-d2"].iter().map(|id| case(id)).collect(),
        unbuildable: vec![],
    }
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
fn the_pool_and_the_endemic_list_read_from_the_wards_own_files() {
    let pool = read_pool(POOL).expect("the pool parses");
    assert_eq!(pool.len(), 60);
    let ploy = person(&pool, "THA-0");
    assert_eq!((ploy.name.as_str(), ploy.sex, ploy.country.as_str(), ploy.place.as_str()), ("Ploy Siriwattana", Sex::F, "THA", "Thailand"));
    assert_eq!(person(&pool, "IDN-2").name, "Agus Pratama");
    let endemic = read_endemic(ENDEMIC).expect("the list parses");
    assert!(endemic.is_empty(), "empty today, and honestly so");
}

#[test]
fn the_same_inputs_and_seed_give_the_same_packs() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, ward, ledger, endemic) = (catalogue(), full_manifest(&pool), empty_ward(), Ledger::default(), BTreeMap::new());
    let i = Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &ward, ledger: &ledger, want: 6, seed: 7 };
    let a = plan(&i);
    let b = plan(&i);
    assert_eq!(a.packs.len(), 6);
    assert_eq!(a.packs, b.packs);
    let c = plan(&Inputs { seed: 8, ..i });
    assert_ne!(a.packs, c.packs, "another seed is another draw");
}

#[test]
fn every_pack_is_one_the_door_would_take_and_agrees_with_its_own_case() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, ward, ledger, endemic) = (catalogue(), full_manifest(&pool), empty_ward(), Ledger::default(), BTreeMap::new());
    let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &ward, ledger: &ledger, want: 20, seed: 1 });
    assert_eq!(p.packs.len(), 20);
    for pl in &p.packs {
        validate_pack(&pl.pack).unwrap_or_else(|why| panic!("{}: the door would refuse her: {why}", pl.pack.persona.name));
        let case = cat.get(&pl.pack.case).expect("a buildable case");
        assert!(case.band.contains(&pl.pack.persona.age), "{}: {} is outside {:?}", case.id, pl.pack.persona.age, case.band);
        let who = person(&pool, &pl.person);
        assert_eq!(who.sex, case.sex, "{}: a {} on a case written for a {}", case.id, who.sex.word(), case.sex.word());
        assert_eq!(pl.pack.persona.name, who.name);
        assert_eq!(pl.pack.persona.country, who.country);
        assert_eq!(pl.sex, who.sex);
    }
}

#[test]
fn nobody_is_on_the_ward_twice() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, endemic) = (catalogue(), full_manifest(&pool), BTreeMap::new());
    let ploy = person(&pool, "THA-0");
    let anan = person(&pool, "THA-1");
    let mut ward = empty_ward();
    ward.patients.push(on_board(1, "on_shift", ploy, "osce-c2", 50));
    ward.patients.push(on_board(2, "went_home", person(&pool, "IDN-1"), "osce-b", 25));
    let mut ledger = Ledger::default();
    ledger.sent.insert("deadbeef".into(), Sent::new("osce-a", anan, 70, false, None, 1, "test"));
    let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &ward, ledger: &ledger, want: 40, seed: 3 });
    let keys: Vec<&str> = p.packs.iter().map(|pl| pl.person.as_str()).collect();
    assert!(!keys.contains(&"THA-0"), "Ploy is in a bed");
    assert!(!keys.contains(&"THA-1"), "Anan is queued and unseen");
    assert!(keys.contains(&"IDN-1"), "Budi went home, so his face is free again");
    let mut dedup = keys.clone();
    dedup.sort_unstable();
    dedup.dedup();
    assert_eq!(dedup.len(), keys.len(), "no face twice in one plan");
}

#[test]
fn the_bands_are_balanced_against_what_the_ward_already_holds() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, endemic) = (catalogue(), full_manifest(&pool), BTreeMap::new());
    let count = |p: &vitals_factory::plan::Plan, band: &str| p.packs.iter().filter(|pl| difficulty_of(&pl.pack.case) == Some(band)).count();

    let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), want: 9, seed: 2 });
    assert_eq!((count(&p, "student"), count(&p, "intern"), count(&p, "resident")), (3, 3, 3));

    // Two interns already in beds: the next two go to the other bands before intern gets another.
    let mut ward = empty_ward();
    ward.patients.push(on_board(1, "on_ward", person(&pool, "JPN-1"), "osce-b", 25));
    ward.patients.push(on_board(2, "on_ward", person(&pool, "KOR-0"), "osce-c2", 50));
    let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &ward, ledger: &Ledger::default(), want: 2, seed: 2 });
    assert_eq!(count(&p, "intern"), 0, "{:?}", p.packs.iter().map(|x| x.pack.case.clone()).collect::<Vec<_>>());
    assert_eq!(count(&p, "student") + count(&p, "resident"), 2);
}

#[test]
fn a_face_already_made_is_used_and_a_missing_one_is_made_at_her_age() {
    let pool = read_pool(POOL).unwrap();
    let endemic = BTreeMap::new();
    // osce-a2 (F 68 → 62–74) and osce-d (M 62 → 56–68): the batch's 63-year-old faces fit both,
    // so every pack carries a stable portrait, needs nothing made, and stays near the face's age.
    let adults = Catalogue { cases: vec![case("osce-a2"), case("osce-d")], unbuildable: vec![] };
    let man = full_manifest(&pool);
    let p = plan(&Inputs { catalogue: &adults, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), want: 5, seed: 4 });
    assert_eq!(p.packs.len(), 5);
    for pl in &p.packs {
        let Base::Have { url, age, .. } = &pl.base else { panic!("{}: a face exists at 63 and fits", pl.person) };
        assert_eq!(pl.pack.portrait.get("stable"), Some(url));
        assert!(*age == 63, "the face that fits is the 63-year-old's");
        assert!((pl.pack.persona.age as i32 - 63).abs() <= 3, "her age stays near the face's: {}", pl.pack.persona.age);
        assert!(pl.person.ends_with("-2"), "index 2 is the 63-year-old of each country: {}", pl.person);
    }
    // osce-b is written for a man of 25 (23–27): no batch face fits, so one is to be made at the
    // age drawn, and the pack goes out without a picture rather than with a wrong one.
    let young = Catalogue { cases: vec![case("osce-b")], unbuildable: vec![] };
    let p = plan(&Inputs { catalogue: &young, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), want: 2, seed: 4 });
    assert_eq!(p.packs.len(), 2);
    for pl in &p.packs {
        assert!(matches!(&pl.base, Base::Make { .. }), "a face at 28 is not a man of 23–27: {:?}", pl.base);
    }
    // A child's case, the same way.
    let child = Catalogue { cases: vec![case("osce-c")], unbuildable: vec![] };
    let p = plan(&Inputs { catalogue: &child, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), want: 2, seed: 4 });
    assert_eq!(p.packs.len(), 2);
    for pl in &p.packs {
        let Base::Make { key, age } = &pl.base else { panic!("no adult face fits a six-year-old") };
        assert_eq!(key, &pl.person);
        assert!(age_band(6).contains(age) && *age == pl.pack.persona.age);
        assert!(pl.pack.portrait.is_empty(), "a missing picture is never a reason to withhold a patient, and never a wrong picture");
    }
    // Once that face is recorded under key@age, the next plan uses it.
    let mut man2 = man.clone();
    let first = &p.packs[0];
    man2.record_base(&first.person, first.pack.persona.age, &url("child"), person(&pool, &first.person));
    let p2 = plan(&Inputs { catalogue: &child, pool: &pool, endemic: &endemic, manifest: &man2, ward: &empty_ward(), ledger: &Ledger::default(), want: 2, seed: 4 });
    let again = p2.packs.iter().find(|pl| pl.person == first.person).expect("same seed, same person");
    assert!(matches!(&again.base, Base::Have { url: u, .. } if u == &url("child")));
}

#[test]
fn pool_exhausted_builds_nothing_and_says_so() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, endemic) = (catalogue(), full_manifest(&pool), BTreeMap::new());
    let mut ledger = Ledger::default();
    for (n, p) in pool.iter().enumerate() {
        ledger.sent.insert(format!("id{n}"), Sent::new("osce-a", p, 70, false, None, 1, "test"));
    }
    let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &ledger, want: 3, seed: 1 });
    assert!(p.packs.is_empty());
    assert!(p.exhausted);
    assert!(p.notes.iter().any(|n| n.contains("pool exhausted")), "{:?}", p.notes);
}

#[test]
fn a_persona_whose_sex_no_case_was_written_for_is_skipped_not_forced() {
    let pool = read_pool(POOL).unwrap();
    let (man, endemic) = (full_manifest(&pool), BTreeMap::new());
    let men_only = Catalogue { cases: vec![case("osce-a")], unbuildable: vec![] };
    let p = plan(&Inputs { catalogue: &men_only, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), want: 60, seed: 1 });
    let men = pool.iter().filter(|x| x.sex == Sex::M).count();
    assert_eq!(p.packs.len(), men, "every man, no woman");
    assert!(p.exhausted, "and then the pool is exhausted for this catalogue");
}

#[test]
fn endemic_is_true_only_when_the_list_pairs_her_country_with_her_case() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man) = (catalogue(), full_manifest(&pool));
    let endemic = read_endemic(r#"{"endemic": {"THA": ["osce-c2", "osce-a"], "IDN": ["osce-b"]}}"#).unwrap();
    let mut endemic_seen = 0;
    for seed in 0..40 {
        let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), want: 12, seed });
        for pl in &p.packs {
            validate_pack(&pl.pack).unwrap_or_else(|why| panic!("{why}"));
            if pl.pack.endemic {
                endemic_seen += 1;
                assert!(endemic[&pl.pack.persona.country].contains(&pl.pack.case), "{:?}", pl.pack);
            }
        }
    }
    assert!(endemic_seen > 0, "one draw in five for a listed country, over forty seeds, is never zero");
    let none = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &BTreeMap::new(), manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), want: 12, seed: 5 });
    assert!(none.packs.iter().all(|pl| !pl.pack.endemic), "no list, no endemic tag");
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

    // Somebody on the board this ledger never sent (another factory, a hand push) is busy too.
    ward.patients.push(on_board(7, "on_shift", person(&pool, "NGA-2"), "osce-d4", 70));
    assert!(ledger.busy_keys(&ward, &pool).contains("NGA-2"));
    let round = Ledger::parse(&ledger.to_json()).unwrap();
    assert_eq!(round, ledger);
}
