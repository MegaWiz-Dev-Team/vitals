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
use vitals_factory::door::{parse_cases, BoardPatient, WardCase, WardView};
use vitals_factory::ledger::{Ledger, Sent};
use vitals_factory::manifest::{batch_age, Manifest};
use vitals_factory::need::{weights, Weights};
use vitals_factory::plan::{bed_cap, plan, Inputs, Plan, MIN_COUNTRIES, MIN_REGIONS, QUEUE_CAP, QUEUE_WINDOW, WORLD_WINDOW};
use vitals_factory::pool::{read_pool, Person};
use vitals_factory::region::{region_of, Region, ALL};
use vitals_factory::sex::Sex;
use vitals_web::ward_chain::PORTRAITS;

const POOL: &str = include_str!("../../vitals-web/data/personas.json");
const PHYSICIANS: &str = include_str!("../../vitals-web/data/physicians.json");
const CASES: &str = include_str!("fixtures/ward-cases-2026-09-16.json");

/// The ward's list as the fixture has it.
fn cases() -> Vec<WardCase> {
    parse_cases(CASES).expect("the fixture parses")
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

/// Twenty-two countries covering every region, every need above four per cent after the largest
/// among them: a queue of these owes no floor, so what the draw does next is need's alone. The
/// two before the last two are what a second round swaps for the largest need in the cap test.
const ROUND: [&str; 22] = [
    "SSD", "SOM", "MWI", "PNG", "BDI", "TCD", "RWA", "YEM", "SEN", "SLE", "TZA", "CMR", "ZWE", "ETH", "MDG", "HTI", "IDN", "BGD", "DEU", "KAZ", "JPN", "USA",
];

/// The country with the most people per doctor in the pool — the largest weight, by the numbers.
fn greatest_need(w: &Weights) -> String {
    w.ranked()[0].0.clone()
}

/// A ledger holding `seq` as packs already sent and still waiting, oldest first.
fn waiting(pool: &[Person], seq: &[&str]) -> Ledger {
    let mut l = Ledger::default();
    let mut used: BTreeSet<String> = BTreeSet::new();
    for (n, c) in seq.iter().enumerate() {
        let who = pool.iter().find(|p| &p.country == c && !used.contains(&p.key)).unwrap_or_else(|| panic!("a free person from {c}"));
        used.insert(who.key.clone());
        l.sent.insert(format!("sent{n:03}"), Sent::new("world-rta-adult", who, 66, false, None, 1000 + n as u64, "test"));
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
    let (cat, man) = (cases(), full_manifest(&pool));
    let w = weights(PHYSICIANS, &pool).unwrap();
    for seed in 0..25 {
        let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &Ledger::default(), weights: &w, beds: 3, want: QUEUE_WINDOW, seed });
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

/// The queue the rules read is the last twenty packs sent, not only this tick's: two of the
/// greatest need already waiting means none of it now, and the tick is not skipped — the draw is
/// done again over the countries still allowed, and the plan says so.
#[test]
fn a_country_at_its_cap_is_redrawn_not_skipped_and_the_plan_says_which() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man) = (cases(), full_manifest(&pool));
    let w = weights(PHYSICIANS, &pool).unwrap();
    let top = greatest_need(&w);
    assert!(!ROUND.contains(&top.as_str()));
    // Forty-four waiting: a round of twenty-two, then the round again with the greatest need in
    // place of two others near its end. That country is two of forty-four — behind its nine per
    // cent, so need draws it — and two of the last nineteen, so the cap refuses it.
    let mut history: Vec<&str> = ROUND.to_vec();
    history.extend(ROUND.iter().map(|c| if *c == "DEU" || *c == "KAZ" { top.as_str() } else { *c }));
    assert_eq!(history.len(), 44);
    let ledger = waiting(&pool, &history);
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &ledger, weights: &w, beds: 3, want: 15, seed: 4 });
    let seq = countries(&p);
    assert_eq!(seq.len(), 15, "never a tick skipped: {:?}", p.notes);
    assert!(!seq.contains(&top.as_str()), "{top} holds two of the last twenty throughout: {seq:?}");
    let whole: Vec<&str> = history.iter().copied().chain(seq.iter().copied()).collect();
    for window in whole.windows(QUEUE_WINDOW) {
        assert!(tally(window).values().all(|n| *n <= QUEUE_CAP), "{:?}", tally(window));
    }
    // Need drew the greatest need first — behind its share — and the redraw took the next country
    // allowed; the plan says so, draw by draw and in one line.
    let first = p.redrawn.iter().find(|r| r.slot == 0).expect("the first draw was redrawn");
    assert_eq!(first.drawn, top);
    assert!(first.why.contains("queue cap"), "{}", first.why);
    assert_eq!(first.took, seq[0]);
    assert!(p.redrawn.iter().filter(|r| r.drawn == top).count() >= 10, "{:?}", p.redrawn);
    assert!(p.notes.iter().any(|n| n.contains("redrawn") && n.contains(&top)), "{:?}", p.notes);
    // Once its two have aged out of the last twenty, it is drawn again: forty-six sent, the
    // greatest need the first two and the round twice after — every region among them, so no
    // floor is owed — leaves it the largest deficit and no longer at the cap.
    let mut older: Vec<&str> = vec![top.as_str(), top.as_str()];
    older.extend(ROUND);
    older.extend(ROUND);
    assert_eq!(older.len(), 46);
    let ledger = waiting(&pool, &older);
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &ledger, weights: &w, beds: 3, want: 3, seed: 4 });
    assert_eq!(countries(&p)[0], top, "its two are forty-six and forty-five packs back: {:?}", countries(&p));
    assert!(p.redrawn.iter().all(|r| r.slot != 0), "{:?}", p.redrawn);
}

/// Three beds: a country in a bed is not queued while another country is available. When every
/// free person is from that country, one is still drawn — the rule yields rather than the tick.
#[test]
fn the_board_never_holds_a_country_twice_while_another_country_is_available() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man) = (cases(), full_manifest(&pool));
    let w = weights(PHYSICIANS, &pool).unwrap();
    // The two greatest needs in two of the three beds, the rest of the round waiting: the
    // greatest is behind its share, so need draws it — and it is in a bed, so the board rule
    // refuses it while seventy-two other countries are free.
    let ranked: Vec<String> = w.ranked().into_iter().map(|(c, _)| c).collect();
    let (top, second) = (ranked[0].as_str(), ranked[1].as_str());
    let mut ward = empty_ward();
    ward.patients.push(on_board(1, person(&pool, &format!("{top}-0")), "world-copd-woman", 66));
    ward.patients.push(on_board(2, person(&pool, &format!("{second}-0")), "world-cholecystitis-woman", 50));
    let others: Vec<&str> = ROUND.iter().copied().filter(|c| *c != second).collect();
    let ledger = waiting(&pool, &others);
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &ward, ledger: &ledger, weights: &w, beds: 3, want: 10, seed: 2 });
    let seq = countries(&p);
    assert_eq!(seq.len(), 10, "{:?}", p.notes);
    assert!(!seq.contains(&top) && !seq.contains(&second), "{seq:?}");
    let first = p.redrawn.iter().find(|r| r.slot == 0).expect("the first draw was redrawn");
    assert!(first.drawn == top && first.why.contains("bed"), "{first:?}");

    // Only people of the country in the bed free: the board rule yields, and the plan says so.
    let one: Vec<Person> = pool.iter().filter(|x| x.country == top).cloned().collect();
    let wf = Weights::flat(&one);
    let p = plan(&Inputs { cases: &cat, pool: &one, manifest: &man, ward: &ward, ledger: &Ledger::default(), weights: &wf, beds: 3, want: 2, seed: 2 });
    assert_eq!(countries(&p), vec![top, top], "{:?}", p.notes);
    assert!(p.notes.iter().any(|n| n.contains("no other country")), "{:?}", p.notes);
}

/// Two hundred draws from an empty ward with the real files, the ward admitting as the factory
/// builds: the queue fills to twenty, then each step the oldest waiting pack is admitted and
/// discharged (the face is free again) and one pack is built to replace it. The country with the
/// most people per doctor has the largest share, no country exceeds its cap in any window of
/// twenty, every region appears in the first forty, and the shares still follow the weights —
/// need decides how often, the spread only where the weights would have left a corner of the
/// world dark.
#[test]
fn over_two_hundred_draws_need_sets_the_shares_and_the_spread_holds() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man) = (cases(), full_manifest(&pool));
    let w = weights(PHYSICIANS, &pool).unwrap();
    let mut ledger = Ledger::default();
    let mut seq: Vec<String> = Vec::new();
    let mut redrawn = Vec::new();
    for step in 0..200u64 {
        // The ward takes the oldest waiting pack once the queue is full; the face frees.
        let waiting: Vec<(String, u64)> = ledger.unseen().into_iter().map(|(id, s)| (id.clone(), s.sent_at)).collect();
        if waiting.len() >= QUEUE_WINDOW {
            let (oldest, _) = waiting.iter().min_by_key(|(id, at)| (*at, id.clone())).unwrap().clone();
            let s = ledger.sent.get_mut(&oldest).unwrap();
            s.patient_id = Some(step + 1);
            s.closed = true;
        }
        let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &empty_ward(), ledger: &ledger, weights: &w, beds: 3, want: 1, seed: 16 + step });
        assert_eq!(p.packs.len(), 1, "step {step}: {:?}", p.notes);
        let pl = &p.packs[0];
        let who = person(&pool, &pl.person);
        // Keyed by step, not pack id: a person freed and drawn again on the same case at the same
        // age is the same pack to the door, and this history must keep both draws.
        ledger.sent.insert(format!("step{step:03}"), Sent::new(&pl.pack.case, who, pl.pack.persona.age, pl.pack.endemic, None, 1000 + step, "test"));
        seq.push(pl.pack.persona.country.clone());
        redrawn.extend(p.redrawn);
    }
    let seq: Vec<&str> = seq.iter().map(String::as_str).collect();
    let t = tally(&seq);
    let top = greatest_need(&w);
    let most = t.values().max().copied().unwrap();
    assert_eq!(t[top.as_str()], most, "{top}, the most people per doctor, is drawn most: {t:?}");
    assert_eq!(t.values().filter(|n| **n == most).count(), 1, "and alone at the top: {t:?}");
    assert!(most <= 200 / QUEUE_WINDOW * QUEUE_CAP, "and the cap of two in twenty bounds it like everyone else: {most}");
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
    // The floors lift every region to at least one patient in forty — five in two hundred — and
    // every redraw that did it is on record, with the country need had drawn.
    let mut per_region: BTreeMap<Region, usize> = BTreeMap::new();
    for c in &seq {
        *per_region.entry(region_of(c).unwrap()).or_default() += 1;
    }
    for r in ALL {
        assert!(per_region.get(&r).copied().unwrap_or(0) >= 200 / WORLD_WINDOW, "{}: {:?}", r.name(), per_region);
    }
    assert!(!redrawn.is_empty() && redrawn.iter().all(|r| r.drawn != r.took && !r.why.is_empty()), "{redrawn:?}");
    // The greatest need is due nearly twice in every twenty, so the cap does meet it now and
    // then — and the floors too; each time the plan says which, and the draw goes to the next
    // need instead.
    let cap_hits = redrawn.iter().filter(|r| r.drawn == top && r.why.contains("queue cap")).count();
    assert!(cap_hits >= 1, "{:?}", redrawn.iter().filter(|r| r.drawn == top).collect::<Vec<_>>());
    eprintln!(
        "SIM {top}: drawn {} of 200 (share {:.1}), {cap_hits} cap redraws, {} floor redraws; {} redraws in all",
        t[top.as_str()],
        w.of(&top) / total * 200.0,
        redrawn.iter().filter(|r| r.drawn == top).count() - cap_hits,
        redrawn.len()
    );
}

/// The bed rule and the queue rule together on a real board: three in beds, sixteen waiting, four
/// to build — none from a bed's country, none beyond two in the last twenty, and the four are
/// four.
#[test]
fn a_tick_against_a_full_board_and_a_deep_queue_still_builds_its_shortfall() {
    let pool = read_pool(POOL).unwrap();
    let (cat, man) = (cases(), full_manifest(&pool));
    let w = weights(PHYSICIANS, &pool).unwrap();
    let mut ward = empty_ward();
    ward.patients.push(on_board(1, person(&pool, "ETH-0"), "world-copd-woman", 66));
    ward.patients.push(on_board(2, person(&pool, "KEN-0"), "world-cholecystitis-woman", 50));
    ward.patients.push(on_board(3, person(&pool, "IDN-0"), "world-copd-woman", 66));
    let queued = ["MDG", "MOZ", "MLI", "UGA", "COD", "AGO", "GHA", "HTI", "ZMB", "NGA", "JAM", "THA", "EGY", "BGD", "MDG", "MOZ"];
    let ledger = waiting(&pool, &queued);
    let p = plan(&Inputs { cases: &cat, pool: &pool, manifest: &man, ward: &ward, ledger: &ledger, weights: &w, beds: 3, want: 4, seed: 8 });
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
