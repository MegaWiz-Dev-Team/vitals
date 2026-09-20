//! Founder, 16 Sep 2026: "ควรมีคนไข้จากทั่วโลกนะ" — the patients must come from the whole world.
//!
//! The pool is seventy-four countries across ten regions, and the crate carries a region table
//! that places every one of them, so the draw can be held to a spread and not only to a weight.
//! The twenty countries of the first spread are untouched — the portrait manifest keys on their
//! positions — and the fifty-four added carry four to six invented people each, both sexes, names
//! plausible for the country's common naming traditions and no name twice anywhere in the pool.
//! The fourteen with the worst shortage of doctors are among them (coordinator, 16 Sep: the
//! mission is to bring that number down, so the pool cannot leave them out to keep a test tidy).

use std::collections::{BTreeMap, BTreeSet};
use vitals_factory::sex::Sex;
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
fn the_pool_spans_the_world_in_every_region_including_the_worst_shortages() {
    let pool = read_pool(POOL).expect("the pool parses");
    let file: serde_json::Value = serde_json::from_str(POOL).unwrap();
    let physicians: serde_json::Value = serde_json::from_str(PHYSICIANS).unwrap();
    let countries: Vec<&str> = file["countries"].as_array().unwrap().iter().map(|c| c["country"].as_str().unwrap()).collect();
    assert_eq!(countries.len(), 74, "seventy-four countries, from the whole world");
    assert_eq!(countries.iter().collect::<BTreeSet<_>>().len(), 74, "no country twice");
    for worst in ["NER", "SSD", "SOM", "TCD", "MWI", "BDI", "SLE", "TZA", "CMR", "ZWE", "SEN", "RWA", "PNG", "YEM"] {
        assert!(countries.contains(&worst), "{worst}: the worst shortages are the mission, and they are in the pool");
    }

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

    // The fifty-four added: four to six people, both sexes, full names, a place.
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
            assert!(name.is_ascii(), "{code}: {name} — `name` is romanised, the home script goes in `local`");
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
    assert_eq!(pool.len(), 400, "one hundred and five of the first spread, and two hundred and ninety-five added");
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

/// A country's `place` is what the globe's atlas polygon is called — nothing else. The ward
/// publishes it as `country_name`, prints it at the bedside ("· from South Korea"), says it on the
/// no-JS pages, and hands it to this factory's portrait prompt (`pool.rs`), so a label the atlas
/// spells another way is a country that reads as two places on one screen. developer-7b corrected
/// the ward's twenty-country file on `cwf/ward` (f9b8e33); this is the seventy-four-country file,
/// which wins at the merge, held to the same rule against the same atlas — the 110m TopoJSON the
/// globe page embeds, joined on ISO3 through its `ALPHA3` table, as every join on that page is.
/// Every mismatch is listed, not the first.
///
/// The rule is what the map *displays as the country's name*, not the raw string in the 110m
/// file: that file abbreviates a few names so a label fits its polygon, and the product says the
/// full name — "from S. Sudan" at a bedside and "a man from Dem. Rep. Congo" in a portrait prompt
/// are both wrong. [`ATLAS_ABBREVIATES`] expands them before the comparison.
#[test]
fn every_place_in_the_pool_is_what_the_globes_atlas_calls_that_country() {
    let page = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../vitals-web/static/world/index.html")).expect("the globe page");
    // The atlas: `<script id="atlas" type="application/json">{…}</script>`, numeric ids.
    let open = "<script id=\"atlas\" type=\"application/json\">";
    let at = page.find(open).expect("the page embeds the atlas") + open.len();
    let end = at + page[at..].find("</script>").expect("the atlas closes");
    let atlas: serde_json::Value = serde_json::from_str(&page[at..end]).expect("the atlas is JSON");
    let mut polygon: BTreeMap<String, String> = BTreeMap::new();
    for g in atlas["objects"]["countries"]["geometries"].as_array().expect("geometries") {
        // Three polygons carry no id (N. Cyprus, Somaliland, Kosovo); nothing in the pool is one.
        if let (Some(id), Some(name)) = (g["id"].as_str(), g["properties"]["name"].as_str()) {
            polygon.insert(id.to_string(), name.to_string());
        }
    }
    assert!(polygon.len() > 150, "the 110m atlas has some 174 named polygons, not {}", polygon.len());
    // The join: `ALPHA3 = {"AFG":"004", …}` — alpha-3 to the atlas's numeric id.
    let at = page.find("ALPHA3 = {").expect("the page joins on an ALPHA3 table") + "ALPHA3 = ".len();
    let end = at + page[at..].find('}').expect("the table closes") + 1;
    let alpha3: BTreeMap<String, String> = serde_json::from_str(&page[at..end]).expect("the ALPHA3 table is JSON");

    let file: serde_json::Value = serde_json::from_str(POOL).unwrap();
    let mut wrong: Vec<String> = Vec::new();
    for c in file["countries"].as_array().unwrap() {
        let code = c["country"].as_str().unwrap();
        let place = c["place"].as_str().unwrap_or("");
        match alpha3.get(code).and_then(|id| polygon.get(id)).map(|n| atlas_name(n)) {
            None => wrong.push(format!("{code}: {place:?} — the atlas has no polygon for {code}, so the globe cannot place it")),
            Some(name) if name != place => wrong.push(format!("{code}: place is {place:?}, the map's name is {name:?}")),
            Some(_) => {}
        }
    }
    assert!(wrong.is_empty(), "{} place label(s) differ from the globe's atlas — `place` is printed at the bedside, said by the no-JS pages and read into the portrait prompt, so it says what the map says:\n  {}", wrong.len(), wrong.join("\n  "));
}

/// The names the 110m atlas abbreviates so a label fits its polygon, and the full name the
/// product says for each — a rendering shortcut of the map file, not the country's name. Whole
/// names, not a prefix rule: "S. " would also rewrite "Fr. S. Antarctic Lands". Only the two that
/// touch the seventy-four are here; the atlas also carries Bosnia and Herz., Central African Rep.,
/// Dominican Rep., W. Sahara, Falkland Is., Eq. Guinea, Solomon Is. and Fr. S. Antarctic Lands
/// (Côte d'Ivoire is spelled out, with its accents), none of them pooled — pooling one fails the
/// test with the abbreviation in the message, which is the prompt to add its row.
const ATLAS_ABBREVIATES: [(&str, &str); 2] = [("S. Sudan", "South Sudan"), ("Dem. Rep. Congo", "Democratic Republic of the Congo")];

/// What the map displays as the country's name: the polygon's, expanded where the atlas abbreviates.
fn atlas_name(polygon: &str) -> String {
    ATLAS_ABBREVIATES.iter().find(|(short, _)| *short == polygon).map_or(polygon, |(_, full)| full).to_string()
}
