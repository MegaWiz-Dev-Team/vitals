//! Founder, 16 Sep 2026: "ควรมีคนไข้จากทั่วโลกนะ" — the patients must come from the whole world.
//!
//! The pool is sixty countries across ten regions, and the crate carries a region table that
//! places every one of them, so the draw can be held to a spread and not only to a weight. The
//! twenty countries of the first spread are untouched — the portrait manifest keys on their
//! positions — and the forty added carry four to six invented people each, both sexes, names
//! plausible for the country's common naming traditions and no name twice anywhere in the pool.

use std::collections::{BTreeMap, BTreeSet};
use vitals_factory::catalogue::Sex;
use vitals_factory::pool::read_pool;
use vitals_factory::region::{region_of, Region, ALL};

const POOL: &str = include_str!("../../vitals-web/data/personas.json");
const PHYSICIANS: &str = include_str!("../../vitals-web/data/physicians.json");

/// The spread as the founder asked for it: every region present, and no region thin.
fn least(region: Region) -> usize {
    match region {
        Region::Europe => 8,
        Region::MiddleEastNorthAfrica => 5,
        Region::SubSaharanAfrica => 10,
        Region::CentralAsiaCaucasus => 3,
        Region::SouthAsia => 4,
        Region::SoutheastAsia => 6,
        Region::EastAsia => 3,
        Region::OceaniaPacific => 3,
        Region::LatinAmericaCaribbean => 8,
        Region::NorthAmerica => 1,
    }
}

#[test]
fn the_pool_spans_sixty_countries_in_every_region() {
    let pool = read_pool(POOL).expect("the pool parses");
    let file: serde_json::Value = serde_json::from_str(POOL).unwrap();
    let physicians: serde_json::Value = serde_json::from_str(PHYSICIANS).unwrap();
    let countries: Vec<&str> = file["countries"].as_array().unwrap().iter().map(|c| c["country"].as_str().unwrap()).collect();
    assert_eq!(countries.len(), 60, "sixty countries, from the whole world");
    assert_eq!(countries.iter().collect::<BTreeSet<_>>().len(), 60, "no country twice");

    // Alpha-3, and only codes the physicians table knows, so every country has a need to weigh by.
    for c in &countries {
        assert_eq!(c.len(), 3, "{c} is not alpha-3");
        assert!(c.chars().all(|ch| ch.is_ascii_uppercase()), "{c} is not alpha-3");
        assert!(physicians["countries"].get(c).is_some(), "{c} is not in physicians.json's country table");
    }

    // Every region, and none thin.
    let mut per_region: BTreeMap<Region, BTreeSet<&str>> = BTreeMap::new();
    for c in &countries {
        let r = region_of(c).unwrap_or_else(|| panic!("{c} has no region"));
        per_region.entry(r).or_default().insert(c);
    }
    for r in ALL {
        let got = per_region.get(&r).map_or(0, BTreeSet::len);
        assert!(got >= least(r), "{}: {got} countries, wanted at least {}", r.name(), least(r));
    }
    assert!(per_region[&Region::OceaniaPacific].iter().any(|c| ["FJI", "PNG", "SLB", "WSM", "TON", "VUT", "KIR", "FSM", "TUV", "MHL", "PLW", "NRU"].contains(c)), "a Pacific island state");
    assert!(per_region[&Region::LatinAmericaCaribbean].iter().any(|c| ["HTI", "JAM", "CUB", "DOM", "TTO", "BRB", "BHS", "LCA", "GRD", "DMA", "VCT", "ATG", "KNA"].contains(c)), "a Caribbean state");

    // The twenty of the first spread are untouched: same order, same depth, same first names —
    // the portrait manifest and the ledger key on the position in the file.
    let first_spread = [
        ("THA", 9), ("IDN", 9), ("PHL", 6), ("VNM", 3), ("MYS", 3), ("MMR", 6), ("IND", 6), ("BGD", 6), ("NPL", 3), ("PAK", 3),
        ("JPN", 3), ("KOR", 3), ("CHN", 3), ("NGA", 9), ("KEN", 9), ("ETH", 9), ("EGY", 6), ("BRA", 3), ("MEX", 3), ("USA", 3),
    ];
    for (n, (code, depth)) in first_spread.iter().enumerate() {
        let c = &file["countries"][n];
        assert_eq!(c["country"].as_str().unwrap(), *code, "position {n} of the file");
        assert_eq!(c["personas"].as_array().unwrap().len(), *depth, "{code} keeps its people");
    }
    assert_eq!(pool.iter().find(|p| p.key == "THA-0").unwrap().name, "Ploy Siriwattana");
    assert_eq!(pool.iter().find(|p| p.key == "ETH-8").unwrap().name, "Samuel Girma");
    assert_eq!(pool.iter().find(|p| p.key == "USA-2").unwrap().name, "Emily Novak");

    // The forty added: four to six people, both sexes, full names, a place.
    for c in file["countries"].as_array().unwrap().iter().skip(20) {
        let code = c["country"].as_str().unwrap();
        let people = c["personas"].as_array().unwrap();
        assert!((4..=6).contains(&people.len()), "{code}: {} people, wanted four to six", people.len());
        assert!(!c["place"].as_str().unwrap_or("").trim().is_empty(), "{code}: a place for the portrait prompt");
        let sexes: BTreeSet<&str> = people.iter().map(|p| p["sex"].as_str().unwrap()).collect();
        assert!(sexes.contains("f") && sexes.contains("m"), "{code}: both sexes");
        for p in people {
            let name = p["name"].as_str().unwrap();
            assert!(name.contains(' '), "{code}: {name} is one word — a chart carries a full name");
            assert!(name.chars().all(|ch| ch.is_ascii()), "{code}: {name} — `name` is romanised, the home script goes in `local`");
            assert!(!p.as_object().unwrap().contains_key("age"), "{code}: {name} carries an age; the case carries the band");
            if let Some(local) = p.get("local") {
                let local = local.as_str().unwrap();
                assert!(!local.trim().is_empty() && local != name, "{code}: {name} — `local` must be the name at home, not a copy");
            }
        }
    }

    // Nobody twice, anywhere; and both sexes in every country (the ward's own test says so too).
    let mut names: Vec<&str> = pool.iter().map(|p| p.name.as_str()).collect();
    let n = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), n, "no name twice in the pool");
    for c in &countries {
        let people: Vec<_> = pool.iter().filter(|p| &p.country == c).collect();
        assert!(people.iter().any(|p| p.sex == Sex::F) && people.iter().any(|p| p.sex == Sex::M), "{c}: both sexes");
    }
    assert_eq!(pool.len(), 323, "one hundred and five of the first spread, and two hundred and eighteen added");
}

#[test]
fn the_region_table_places_every_country_in_the_pool_and_names_ten_regions() {
    let pool = read_pool(POOL).unwrap();
    let countries: BTreeSet<&str> = pool.iter().map(|p| p.country.as_str()).collect();
    for c in &countries {
        assert!(region_of(c).is_some(), "{c} is in the pool and the region table does not place it");
    }
    assert_eq!(ALL.len(), 10);
    let names: BTreeSet<&str> = ALL.iter().map(|r| r.name()).collect();
    assert_eq!(names.len(), 10, "ten regions, ten names");
    assert_eq!(region_of("ETH"), Some(Region::SubSaharanAfrica));
    assert_eq!(region_of("EGY"), Some(Region::MiddleEastNorthAfrica));
    assert_eq!(region_of("THA"), Some(Region::SoutheastAsia));
    assert_eq!(region_of("USA"), Some(Region::NorthAmerica));
    assert_eq!(region_of("ATA"), None, "a code the table does not place is nobody's region, not a guess");
    assert_eq!(region_of("eth"), None, "alpha-3 is upper case");
}
