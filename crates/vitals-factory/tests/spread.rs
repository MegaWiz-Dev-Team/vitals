//! Need decides how often; diversity decides the board.
//!
//! Founder, 16 Sep 2026: "ควรมีคนไข้จากทั่วโลกนะ" — and, the same morning, that the patients must
//! reflect real need. The two rules meet in `plan()`: the country is still drawn by people per
//! doctor, so twice the need is twice the patients over a run; but the queue the ward admits from
//! (the last twenty packs sent) holds no country more than twice, shows at least twelve countries
//! and six regions, and no region waits more than forty draws. When need draws a country the
//! spread will not take, the draw is done again over the countries still allowed — never a tick
//! skipped — and the plan says which countries were redrawn and why. The board (three beds)
//! never holds a country twice while another country is available.

use std::collections::{BTreeMap, BTreeSet};
use vitals_factory::catalogue::{Case, Catalogue, Sex};
use vitals_factory::door::{BoardPatient, WardView};
use vitals_factory::ledger::{Ledger, Sent};
use vitals_factory::manifest::{batch_age, Manifest};
use vitals_factory::need::{weights, Weights};
use vitals_factory::plan::{bed_cap, plan, Inputs, Plan, MIN_COUNTRIES, MIN_REGIONS, QUEUE_CAP, QUEUE_WINDOW, WORLD_WINDOW};
use vitals_factory::pool::{read_pool, Person};
use vitals_factory::region::{region_of, Region, ALL};
use vitals_web::ward::{age_band, case_patient, difficulty_of};
use vitals_web::ward_chain::PORTRAITS;

const POOL: &str = include_str!("../../vitals-web/data/personas.json");
const PHYSICIANS: &str = include_str!("../../vitals-web/data/physicians.json");

fn case(id: &str) -> Case {
    let theirs = case_patient(id).expect("a station");
    Case { id: id.into(), sex: Sex::parse(&theirs.sex).unwrap(), band: age_band(theirs.age), difficulty: difficulty_of(id).expect("a ward case"), source: "test".into() }
}

/// Every band, both sexes.
fn catalogue() -> Catalogue {
    Catalogue { cases: ["osce-a", "osce-a2", "osce-b", "osce-c2", "osce-c", "osce-d4", "osce-d2"].iter().map(|id| case(id)).collect(), unbuildable: vec![] }
}

fn url(tag: &str) -> String {
    let mut h = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut h, tag.as_bytes());
    let sha: String = sha2::Digest::finalize(h).iter().map(|b| format!("{b:02x}")).collect();
    format!("{PORTRAITS}/{sha}.webp")
}

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

fn on_board(id: u64, p: &Person, case: &str, age: u16) -> BoardPatient {
    BoardPatient {
        patient_id: id, state: "on_ward".into(), bed: Some(1), name: Some(p.name.clone()), age: Some(age),
        country: Some(p.country.clone()), case: Some(case.into()), endemic: false, portrait: None, portraits: BTreeMap::new(),
    }
}

fn person<'a>(pool: &'a [Person], key: &str) -> &'a Person {
    pool.iter().find(|p| p.key == key).expect(key)
}

fn countries(p: &Plan) -> Vec<&str> {
    p.packs.iter().map(|pl| pl.pack.persona.country.as_str()).collect()
}

fn tally<'a>(seq: &[&'a str]) -> BTreeMap<&'a str, usize> {
    let mut m = BTreeMap::new();
    for c in seq {
        *m.entry(*c).or_default() += 1;
    }
    m
}

fn regions(seq: &[&str]) -> BTreeSet<Region> {
    seq.iter().filter_map(|c| region_of(c)).collect()
}

/// A ledger holding `seq` as packs already sent and still waiting, oldest first.
fn waiting(pool: &[Person], seq: &[&str]) -> Ledger {
    let mut l = Ledger::default();
    let mut used: BTreeSet<String> = BTreeSet::new();
    for (n, c) in seq.iter().enumerate() {
        let who = pool.iter().find(|p| &p.country == c && !used.contains(&p.key)).unwrap_or_else(|| panic!("a free person from {c}"));
        used.insert(who.key.clone());
        l.sent.insert(format!("sent{n:03}"), Sent::new("osce-a2", who, 66, false, None, 1000 + n as u64, "test"));
    }
    l
}

#[test]
fn the_constants_are_the_founders_numbers() {
    assert_eq!((QUEUE_WINDOW, QUEUE_CAP, MIN_COUNTRIES, MIN_REGIONS, WORLD_WINDOW), (20, 2, 12, 6, 40));
    assert_eq!(bed_cap(3), 1, "three beds: no country twice on the board");
    assert!(MIN_COUNTRIES <= QUEUE_WINDOW && MIN_REGIONS <= ALL.len());
}

/// The queue of twenty, from an empty ward and the real weights: no country more than twice, at
/// least twelve countries, at least six regions — over many seeds, since the seed breaks ties.
#[test]
fn the_queue_holds_no_country_twice_over_and_shows_twelve_countries_in_six_regions() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, endemic) = (catalogue(), full_manifest(&pool), BTreeMap::new());
    let w = weights(PHYSICIANS, &pool).unwrap();
    for seed in 0..25 {
        let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &w, beds: 3, want: QUEUE_WINDOW, seed });
        let seq = countries(&p);
        assert_eq!(seq.len(), QUEUE_WINDOW, "seed {seed}: {:?}", p.notes);
        let t = tally(&seq);
        assert!(t.values().all(|n| *n <= QUEUE_CAP), "seed {seed}: {t:?}");
        assert!(t.len() >= MIN_COUNTRIES, "seed {seed}: {} countries in {seq:?}", t.len());
        assert!(regions(&seq).len() >= MIN_REGIONS, "seed {seed}: {} regions in {seq:?}", regions(&seq).len());
        for pl in &p.packs {
            assert_eq!(pl.region, region_of(&pl.pack.persona.country), "each pack carries its region");
        }
    }
}

/// The queue the rules read is the last twenty packs sent, not only this tick's: two of Ethiopia
/// already waiting means none of Ethiopia now, and the tick is not skipped — the draw is done
/// again over the countries still allowed, and the plan says so.
#[test]
fn a_country_at_its_cap_is_redrawn_not_skipped_and_the_plan_says_which() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, endemic) = (catalogue(), full_manifest(&pool), BTreeMap::new());
    let w = weights(PHYSICIANS, &pool).unwrap();
    let ledger = waiting(&pool, &["ETH", "MDG", "ETH", "MOZ", "KEN"]);
    let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &ledger, weights: &w, beds: 3, want: 15, seed: 4 });
    let seq = countries(&p);
    assert_eq!(seq.len(), 15, "never a tick skipped: {:?}", p.notes);
    assert!(!seq.contains(&"ETH"), "Ethiopia holds two of the last twenty already: {seq:?}");
    let whole: Vec<&str> = ["ETH", "MDG", "ETH", "MOZ", "KEN"].into_iter().chain(seq.iter().copied()).collect();
    assert!(tally(&whole).values().all(|n| *n <= QUEUE_CAP), "{:?}", tally(&whole));
    // Need drew Ethiopia first — the greatest need — and the redraw took the next allowed.
    let first = p.redrawn.iter().find(|r| r.slot == 0).expect("the first draw was redrawn");
    assert_eq!(first.drawn, "ETH");
    assert!(first.why.contains("queue cap"), "{}", first.why);
    assert_eq!(first.took, seq[0]);
    assert!(p.notes.iter().any(|n| n.contains("redrawn") && n.contains("ETH")), "{:?}", p.notes);
    // Once the two of Ethiopia have aged out of the last twenty, Ethiopia is drawn again: forty
    // sent, Ethiopia the first two and nineteen others twice each after — every region among
    // them, so no floor is owed — leaves Ethiopia the largest deficit and no longer at the cap.
    let round = ["MDG", "MOZ", "MLI", "UGA", "COD", "AGO", "GHA", "KEN", "HTI", "ZMB", "NGA", "IDN", "EGY", "BGD", "DEU", "KAZ", "JPN", "FJI", "USA"];
    let mut older: Vec<&str> = vec!["ETH", "ETH"];
    older.extend(round);
    older.extend(round);
    assert_eq!(older.len(), 40);
    let ledger = waiting(&pool, &older);
    let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &ledger, weights: &w, beds: 3, want: 3, seed: 4 });
    assert_eq!(countries(&p)[0], "ETH", "the two of Ethiopia are forty and thirty-nine packs back: {:?}", countries(&p));
    assert!(p.redrawn.iter().all(|r| r.slot != 0), "{:?}", p.redrawn);
}

/// Three beds: a country in a bed is not queued while another country is available. When every
/// free person is from that country, one is still drawn — the rule yields rather than the tick.
#[test]
fn the_board_never_holds_a_country_twice_while_another_country_is_available() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, endemic) = (catalogue(), full_manifest(&pool), BTreeMap::new());
    let w = weights(PHYSICIANS, &pool).unwrap();
    let mut ward = empty_ward();
    ward.patients.push(on_board(1, person(&pool, "ETH-0"), "osce-a2", 66));
    ward.patients.push(on_board(2, person(&pool, "MDG-0"), "osce-c2", 50));
    let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &ward, ledger: &Ledger::default(), weights: &w, beds: 3, want: 10, seed: 2 });
    let seq = countries(&p);
    assert_eq!(seq.len(), 10);
    assert!(!seq.contains(&"ETH") && !seq.contains(&"MDG"), "{seq:?}");
    assert!(p.redrawn.iter().any(|r| r.drawn == "ETH" && r.why.contains("bed")), "{:?}", p.redrawn);

    // Only Ethiopians free: the board rule yields, and the plan says so.
    let ethiopians: Vec<Person> = pool.iter().filter(|x| x.country == "ETH").cloned().collect();
    let wf = Weights::flat(&ethiopians);
    let p = plan(&Inputs { catalogue: &cat, pool: &ethiopians, endemic: &endemic, manifest: &man, ward: &ward, ledger: &Ledger::default(), weights: &wf, beds: 3, want: 2, seed: 2 });
    assert_eq!(countries(&p), vec!["ETH", "ETH"], "{:?}", p.notes);
    assert!(p.notes.iter().any(|n| n.contains("no other country")), "{:?}", p.notes);
}

/// Two hundred draws from an empty ward with the real files: Ethiopia's share is the largest, no
/// country exceeds its cap in any window of twenty, every region appears in the first forty, and
/// the shares still follow the weights — need decides how often, the spread only where the
/// weights would have left a corner of the world dark.
#[test]
fn over_two_hundred_draws_need_sets_the_shares_and_the_spread_holds() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, endemic) = (catalogue(), full_manifest(&pool), BTreeMap::new());
    let w = weights(PHYSICIANS, &pool).unwrap();
    let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &w, beds: 60, want: 200, seed: 16 });
    let seq = countries(&p);
    assert_eq!(seq.len(), 200, "{:?}", p.notes);
    let t = tally(&seq);
    let most = t.values().max().copied().unwrap();
    assert_eq!(t["ETH"], most, "Ethiopia, the greatest need, is drawn most: {t:?}");
    assert_eq!(t.values().filter(|n| **n == most).count(), 1, "and alone at the top: {t:?}");
    for window in seq.windows(QUEUE_WINDOW) {
        let tw = tally(window);
        assert!(tw.values().all(|n| *n <= QUEUE_CAP), "{tw:?} in {window:?}");
        assert!(tw.len() >= MIN_COUNTRIES, "{} countries in {window:?}", tw.len());
        assert!(regions(window).len() >= MIN_REGIONS, "{} regions in {window:?}", regions(window).len());
    }
    for window in seq.windows(WORLD_WINDOW) {
        assert_eq!(regions(window).len(), ALL.len(), "a region waited more than forty draws: {window:?}");
    }
    assert_eq!(regions(&seq[..WORLD_WINDOW]).len(), ALL.len(), "every region in the first forty");
    // Shares follow the weights: each country within three of its share of two hundred, and the
    // five most drawn are among the six greatest needs.
    let total: f64 = w.by_country.values().sum();
    for (c, wc) in &w.by_country {
        let expected = wc / total * 200.0;
        let got = t.get(c.as_str()).copied().unwrap_or(0) as f64;
        assert!((got - expected).abs() <= 3.0, "{c}: drawn {got}, share of two hundred is {expected:.1}");
    }
    let mut by_count: Vec<(&str, usize)> = t.iter().map(|(c, n)| (*c, *n)).collect();
    by_count.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let top6: BTreeSet<String> = w.ranked().into_iter().take(6).map(|(c, _)| c).collect();
    for (c, _) in by_count.iter().take(5) {
        assert!(top6.contains(*c), "{c} is among the five most drawn but not the six greatest needs: {by_count:?}");
    }
    assert!(p.redrawn.iter().any(|r| r.drawn == "ETH"), "Ethiopia met the cap somewhere in two hundred: {:?}", p.redrawn.len());
}

/// The bed rule and the queue rule together on a real board: three in beds, sixteen waiting, four
/// to build — none from a bed's country, none beyond two in the last twenty, and the four are
/// four.
#[test]
fn a_tick_against_a_full_board_and_a_deep_queue_still_builds_its_shortfall() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man, endemic) = (catalogue(), full_manifest(&pool), BTreeMap::new());
    let w = weights(PHYSICIANS, &pool).unwrap();
    let mut ward = empty_ward();
    ward.patients.push(on_board(1, person(&pool, "ETH-0"), "osce-a2", 66));
    ward.patients.push(on_board(2, person(&pool, "KEN-0"), "osce-c2", 50));
    ward.patients.push(on_board(3, person(&pool, "IDN-0"), "osce-a2", 66));
    let queued = ["MDG", "MOZ", "MLI", "UGA", "COD", "AGO", "GHA", "HTI", "ZMB", "NGA", "JAM", "THA", "EGY", "BGD", "MDG", "MOZ"];
    let ledger = waiting(&pool, &queued);
    let p = plan(&Inputs { catalogue: &cat, pool: &pool, endemic: &endemic, manifest: &man, ward: &ward, ledger: &ledger, weights: &w, beds: 3, want: 4, seed: 8 });
    let seq = countries(&p);
    assert_eq!(seq.len(), 4, "{:?}", p.notes);
    for c in ["ETH", "KEN", "IDN"] {
        assert!(!seq.contains(&c), "{c} is in a bed: {seq:?}");
    }
    for c in ["MDG", "MOZ"] {
        assert!(!seq.contains(&c), "{c} is twice in the queue: {seq:?}");
    }
    let whole: Vec<&str> = queued.iter().copied().chain(seq.iter().copied()).collect();
    assert!(tally(&whole).values().all(|n| *n <= QUEUE_CAP));
}
