//! Where the need is: people per doctor, per pooled country, as the weight of the draw.
//!
//! Founder, 16 Sep 2026: the ward's patients must reflect real need. The number is the same one
//! the globe colours by — World Bank SH.MED.PHYS.ZS, physicians per 1,000 people, WHO's series
//! republished, read from `crates/vitals-web/data/physicians.json` — as people per doctor,
//! 1000 / per_1000 to the nearest ten, each country at its own latest year.
//!
//! Two guards keep the weighting from becoming a rule about people: a floor at the pooled
//! countries' median ÷ 4, so a country with many doctors still appears; and the median for a
//! country the series has no value for, so missing data is not read as no need.

use crate::pool::Person;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Weights {
    /// Country → weight, floored. Every pooled country has an entry.
    pub by_country: BTreeMap<String, f64>,
    /// People per doctor as published, before the floor, where the series has a value.
    pub people: BTreeMap<String, u32>,
    pub year: BTreeMap<String, u16>,
    /// The median of the pooled countries that have a value.
    pub median: f64,
    /// median ÷ 4.
    pub floor: f64,
}

/// people per doctor = 1000 / physicians per 1,000, to the nearest ten — the globe's own rule.
fn people_per_doctor(per_1000: f64) -> Option<u32> {
    if per_1000 <= 0.0 || !per_1000.is_finite() {
        return None;
    }
    Some(((1000.0 / per_1000 / 10.0).round() * 10.0) as u32)
}

/// The weights for the countries in this pool, from the physicians file.
pub fn weights(physicians_json: &str, pool: &[Person]) -> Result<Weights, String> {
    #[derive(serde::Deserialize)]
    struct Entry {
        series: Vec<(u16, f64)>,
    }
    #[derive(serde::Deserialize)]
    struct File {
        countries: BTreeMap<String, Entry>,
    }
    let file: File = serde_json::from_str(physicians_json).map_err(|e| format!("physicians.json: {e}"))?;
    let mut countries: Vec<String> = pool.iter().map(|p| p.country.clone()).collect();
    countries.sort();
    countries.dedup();

    let mut people = BTreeMap::new();
    let mut year = BTreeMap::new();
    for c in &countries {
        let latest = file
            .countries
            .get(c)
            .and_then(|e| e.series.iter().filter(|(_, v)| *v > 0.0).max_by_key(|(y, _)| *y))
            .copied();
        if let Some((y, v)) = latest {
            if let Some(n) = people_per_doctor(v) {
                people.insert(c.clone(), n);
                year.insert(c.clone(), y);
            }
        }
    }
    let mut known: Vec<f64> = people.values().map(|n| f64::from(*n)).collect();
    known.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    let median = match known.len() {
        0 => return Err("no pooled country has a physicians value, so there is nothing to weight by".into()),
        n if n % 2 == 1 => known[n / 2],
        n => (known[n / 2 - 1] + known[n / 2]) / 2.0,
    };
    let floor = median / 4.0;
    let by_country = countries
        .iter()
        .map(|c| (c.clone(), people.get(c).map_or(median, |n| f64::from(*n)).max(floor)))
        .collect();
    Ok(Weights { by_country, people, year, median, floor })
}

impl Weights {
    /// Every pooled country at one weight — for tests of the other rules, and for a ward that
    /// chooses not to weight.
    pub fn flat(pool: &[Person]) -> Weights {
        let by_country: BTreeMap<String, f64> = pool.iter().map(|p| (p.country.clone(), 1.0)).collect();
        Weights { by_country, people: BTreeMap::new(), year: BTreeMap::new(), median: 1.0, floor: 1.0 }
    }

    /// A weight table written out, for tests.
    pub fn from_table(rows: &[(&str, f64)]) -> Weights {
        let by_country: BTreeMap<String, f64> = rows.iter().map(|(c, w)| (c.to_string(), *w)).collect();
        let mut v: Vec<f64> = by_country.values().copied().collect();
        v.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        let median = v.get(v.len() / 2).copied().unwrap_or(1.0);
        Weights { by_country, people: BTreeMap::new(), year: BTreeMap::new(), median, floor: median / 4.0 }
    }

    /// The weight of a country; the median for one the table does not know.
    pub fn of(&self, country: &str) -> f64 {
        self.by_country.get(country).copied().unwrap_or(self.median)
    }

    pub fn year(&self, country: &str) -> Option<u16> {
        self.year.get(country).copied()
    }

    /// Highest need first; ties by code, so the order is the same everywhere.
    pub fn ranked(&self) -> Vec<(String, f64)> {
        let mut v: Vec<(String, f64)> = self.by_country.iter().map(|(c, w)| (c.clone(), *w)).collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).expect("finite").then_with(|| a.0.cmp(&b.0)));
        v
    }

    /// One line for the log: every weight the tick used, and the floor it used.
    pub fn table(&self) -> String {
        let cells: Vec<String> = self
            .ranked()
            .iter()
            .map(|(c, w)| {
                let floored = self.people.get(c).is_some_and(|n| f64::from(*n) < *w);
                let unknown = !self.people.contains_key(c);
                format!("{c} {}{}", fmt(*w), if floored { "↑" } else if unknown { "?" } else { "" })
            })
            .collect();
        format!(
            "weights: {} (people per doctor, World Bank/WHO latest year; ↑ lifted to the floor, ? no value so the median) · median {} · floor {} = median ÷ 4",
            cells.join(" · "),
            fmt(self.median),
            fmt(self.floor)
        )
    }
}

/// 6990 → "6,990"; 281.25 → "281".
pub fn fmt(w: f64) -> String {
    let n = w.round() as u64;
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
