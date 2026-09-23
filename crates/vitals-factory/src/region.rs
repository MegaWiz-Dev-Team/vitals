//! Where in the world a country is, so the queue can be held to a spread.
//!
//! Founder, 16 Sep 2026: "ควรมีคนไข้จากทั่วโลกนะ" — patients from the whole world. Need (people per
//! doctor, [`crate::need`]) decides how often a country appears over time; this table is what
//! lets the draw also promise that the board and the queue show the world and not one corner of
//! it. Ten regions, chosen so that every inhabited continent is present and the large ones are
//! split where medicine and naming differ: Africa north and south of the Sahara, Asia into four,
//! the Americas into two with the Caribbean beside Latin America.
//!
//! Static and keyed by ISO 3166-1 alpha-3, the same codes `personas.json` and `physicians.json`
//! use. A country in the pool that this table cannot place fails `tests/world.rs`, so adding a
//! country is adding a line here as well as people there. Two placements were a decision rather
//! than a fact: Türkiye is read as Middle East and North Africa (Anatolia is Western Asia), and
//! Egypt and Yemen likewise, with the rest of Africa south of the Sahara.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Region {
    Europe,
    MiddleEastNorthAfrica,
    SubSaharanAfrica,
    CentralAsiaCaucasus,
    SouthAsia,
    SoutheastAsia,
    EastAsia,
    OceaniaPacific,
    LatinAmericaCaribbean,
    NorthAmerica,
}

/// Every region, in the order the tables print them.
pub const ALL: [Region; 10] = [
    Region::Europe,
    Region::MiddleEastNorthAfrica,
    Region::SubSaharanAfrica,
    Region::CentralAsiaCaucasus,
    Region::SouthAsia,
    Region::SoutheastAsia,
    Region::EastAsia,
    Region::OceaniaPacific,
    Region::LatinAmericaCaribbean,
    Region::NorthAmerica,
];

impl Region {
    /// The name a log or a table prints.
    pub fn name(self) -> &'static str {
        match self {
            Region::Europe => "Europe",
            Region::MiddleEastNorthAfrica => "Middle East & North Africa",
            Region::SubSaharanAfrica => "Sub-Saharan Africa",
            Region::CentralAsiaCaucasus => "Central Asia & Caucasus",
            Region::SouthAsia => "South Asia",
            Region::SoutheastAsia => "Southeast Asia",
            Region::EastAsia => "East Asia",
            Region::OceaniaPacific => "Oceania & Pacific",
            Region::LatinAmericaCaribbean => "Latin America & Caribbean",
            Region::NorthAmerica => "North America",
        }
    }
}

/// Alpha-3 → region, for every country in the pool.
pub const TABLE: &[(&str, Region)] = &[
    // Europe: west, north, east, south.
    ("DEU", Region::Europe),
    ("FRA", Region::Europe),
    ("GBR", Region::Europe),
    ("SWE", Region::Europe),
    ("FIN", Region::Europe),
    ("POL", Region::Europe),
    ("UKR", Region::Europe),
    ("ROU", Region::Europe),
    ("ITA", Region::Europe),
    ("ESP", Region::Europe),
    ("GRC", Region::Europe),
    // Middle East and North Africa.
    ("EGY", Region::MiddleEastNorthAfrica),
    ("YEM", Region::MiddleEastNorthAfrica),
    ("MAR", Region::MiddleEastNorthAfrica),
    ("TUN", Region::MiddleEastNorthAfrica),
    ("IRQ", Region::MiddleEastNorthAfrica),
    ("IRN", Region::MiddleEastNorthAfrica),
    ("SAU", Region::MiddleEastNorthAfrica),
    ("TUR", Region::MiddleEastNorthAfrica),
    // Sub-Saharan Africa: west, east, central, south — the fourteen worst shortages among them.
    ("NGA", Region::SubSaharanAfrica),
    ("GHA", Region::SubSaharanAfrica),
    ("MLI", Region::SubSaharanAfrica),
    ("NER", Region::SubSaharanAfrica),
    ("SEN", Region::SubSaharanAfrica),
    ("SLE", Region::SubSaharanAfrica),
    ("KEN", Region::SubSaharanAfrica),
    ("ETH", Region::SubSaharanAfrica),
    ("UGA", Region::SubSaharanAfrica),
    ("MOZ", Region::SubSaharanAfrica),
    ("MDG", Region::SubSaharanAfrica),
    ("SSD", Region::SubSaharanAfrica),
    ("SOM", Region::SubSaharanAfrica),
    ("TZA", Region::SubSaharanAfrica),
    ("RWA", Region::SubSaharanAfrica),
    ("BDI", Region::SubSaharanAfrica),
    ("COD", Region::SubSaharanAfrica),
    ("AGO", Region::SubSaharanAfrica),
    ("CMR", Region::SubSaharanAfrica),
    ("TCD", Region::SubSaharanAfrica),
    ("ZAF", Region::SubSaharanAfrica),
    ("ZMB", Region::SubSaharanAfrica),
    ("ZWE", Region::SubSaharanAfrica),
    ("MWI", Region::SubSaharanAfrica),
    // Central Asia and the Caucasus.
    ("KAZ", Region::CentralAsiaCaucasus),
    ("UZB", Region::CentralAsiaCaucasus),
    ("GEO", Region::CentralAsiaCaucasus),
    // South Asia.
    ("IND", Region::SouthAsia),
    ("BGD", Region::SouthAsia),
    ("NPL", Region::SouthAsia),
    ("PAK", Region::SouthAsia),
    // Southeast Asia.
    ("THA", Region::SoutheastAsia),
    ("IDN", Region::SoutheastAsia),
    ("PHL", Region::SoutheastAsia),
    ("VNM", Region::SoutheastAsia),
    ("MYS", Region::SoutheastAsia),
    ("MMR", Region::SoutheastAsia),
    // East Asia.
    ("JPN", Region::EastAsia),
    ("KOR", Region::EastAsia),
    ("CHN", Region::EastAsia),
    // Oceania and the Pacific.
    ("AUS", Region::OceaniaPacific),
    ("NZL", Region::OceaniaPacific),
    ("FJI", Region::OceaniaPacific),
    ("PNG", Region::OceaniaPacific),
    // Latin America and the Caribbean.
    ("BRA", Region::LatinAmericaCaribbean),
    ("MEX", Region::LatinAmericaCaribbean),
    ("COL", Region::LatinAmericaCaribbean),
    ("PER", Region::LatinAmericaCaribbean),
    ("ARG", Region::LatinAmericaCaribbean),
    ("GTM", Region::LatinAmericaCaribbean),
    ("BOL", Region::LatinAmericaCaribbean),
    ("HTI", Region::LatinAmericaCaribbean),
    ("JAM", Region::LatinAmericaCaribbean),
    // North America.
    ("USA", Region::NorthAmerica),
    ("CAN", Region::NorthAmerica),
];

/// The region of an alpha-3 code, or `None` for one the table does not place. Never a guess: a
/// country without a region counts toward no region in the draw's spread and is never forced.
pub fn region_of(code: &str) -> Option<Region> {
    TABLE.iter().find(|(c, _)| *c == code).map(|(_, r)| *r)
}
