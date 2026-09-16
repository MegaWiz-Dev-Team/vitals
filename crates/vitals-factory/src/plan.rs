//! Building packs: a pure draw over what the factory holds.
//!
//! In: the catalogue (who each case was written for), the pool (who a patient can be), the faces
//! already made, the ward's last answer and the ledger of what was sent. Out: packs the door will
//! take, each with the person it is for and whether her face exists or must be made. No clock, no
//! network, no randomness but the seed — the same inputs give the same packs, which is what makes
//! a dry run a statement about the real one.
//!
//! The rules, in the order they are applied to each pack:
//!
//!   1. **a person, once.** Nobody whose face is on the ward or waiting for a bed is drawn again
//!      ([`crate::ledger::Ledger::busy_keys`]).
//!   2. **where the need is.** The country is drawn by weight — people per doctor, from
//!      [`crate::need`] — by largest deficit against its share of everyone on the ward and
//!      waiting, so twice the need is twice the patients, exactly, over a run; a country in
//!      40 % or more of the beds is not drawn until somebody leaves; and "least on the ward
//!      first" breaks ties within the weighting, never overrides it.
//!   3. **her disease is not her origin.** Four draws in five ignore where she is from. One in
//!      [`vitals_web::ward::ENDEMIC_IN`], for a country with an endemic list, takes a case from
//!      that list — and only then is the pack `endemic`.
//!   4. **the bands are balanced** against what is in beds and what is waiting: the level with
//!      fewest patients is filled first, and within it the case least used.
//!   5. **her sex is the case's**, checked before the case is chosen rather than after; a person
//!      for whom no case fits is skipped, never forced.
//!   6. **her age is inside the case's band** — the door's band — and near her face's age when a
//!      face already fits, so the picture and the number agree. When no face fits, one is to be
//!      made at the age drawn, and the pack goes out without a picture rather than with a wrong
//!      one.

use crate::catalogue::{Case, Catalogue, Sex};
use crate::door::WardView;
use crate::ledger::Ledger;
use crate::manifest::Manifest;
use crate::need::Weights;
use crate::pool::Person;
use std::collections::{BTreeMap, BTreeSet};
use vitals_web::ward::{difficulty_of, Pack, Persona, ENDEMIC_IN};

/// The three levels, in the ward's order.
pub const BANDS: [&str; 3] = ["student", "intern", "resident"];

/// How far from a fitting face's age a pack's age may drift, so the picture and the number agree.
pub const NEAR_FACE: u16 = 3;

#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub catalogue: &'a Catalogue,
    pub pool: &'a [Person],
    pub endemic: &'a BTreeMap<String, Vec<String>>,
    pub manifest: &'a Manifest,
    pub ward: &'a WardView,
    pub ledger: &'a Ledger,
    /// People per doctor per pooled country: the weight of the country draw.
    pub weights: &'a Weights,
    /// The ward's beds, for the cap: no country in more than 40 % of them at once.
    pub beds: usize,
    /// How many packs to build.
    pub want: usize,
    pub seed: u64,
}

/// The share of the beds one country may hold at once.
pub const BED_SHARE: f64 = 0.4;

/// The most beds one country may hold at once, never fewer than one.
pub fn bed_cap(beds: usize) -> usize {
    ((beds as f64 * BED_SHARE).floor() as usize).max(1)
}

/// Where her `stable` picture comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base {
    /// A face that fits the band exists; the pack carries it.
    Have { key: String, url: String, age: u16 },
    /// None fits; one is to be made at this age and recorded under `<key>@<age>`.
    Make { key: String, age: u16 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub pack: Pack,
    /// `<ISO3>-<index>`.
    pub person: String,
    pub sex: Sex,
    /// For the portrait prompt.
    pub place: String,
    pub base: Base,
    /// The weight her country was drawn with: people per doctor, floored.
    pub weight: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub packs: Vec<Planned>,
    pub notes: Vec<String>,
    /// True when a pack was wanted and nobody was left to be it.
    pub exhausted: bool,
}

/// xorshift64*, seeded. Enough for a draw, and the same everywhere.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// A stable per-key shuffle value for one seed and one slot.
fn shuffle_key(seed: u64, slot: usize, key: &str) -> u64 {
    let mut r = Rng::new(seed ^ (slot as u64).wrapping_mul(0xA24B_AED4_963E_E407));
    for b in key.bytes() {
        r.0 ^= u64::from(b);
        r.next();
    }
    r.next()
}

/// What is on the ward or waiting, tallied three ways.
#[derive(Default)]
struct Load {
    band: BTreeMap<&'static str, usize>,
    case: BTreeMap<String, usize>,
    country: BTreeMap<String, usize>,
}

impl Load {
    fn count(&mut self, case: &str, country: &str) {
        if let Some(b) = difficulty_of(case) {
            *self.band.entry(b).or_default() += 1;
        }
        *self.case.entry(case.to_string()).or_default() += 1;
        *self.country.entry(country.to_string()).or_default() += 1;
    }
    fn band(&self, b: &str) -> usize {
        self.band.get(b).copied().unwrap_or(0)
    }
    fn case(&self, c: &str) -> usize {
        self.case.get(c).copied().unwrap_or(0)
    }
    fn country(&self, c: &str) -> usize {
        self.country.get(c).copied().unwrap_or(0)
    }
}

pub fn plan(i: &Inputs) -> Plan {
    let mut out = Plan::default();
    let mut rng = Rng::new(i.seed);

    // What is already on the ward or waiting, by person, band and case.
    let mut busy: BTreeSet<String> = i.ledger.busy_keys(i.ward, i.pool);
    let mut load = Load::default();
    for p in i.ward.open() {
        if let (Some(case), Some(country)) = (&p.case, &p.country) {
            load.count(case, country);
        }
    }
    for (_, s) in i.ledger.unseen() {
        load.count(&s.case, &s.country);
    }

    // Who is in a bed right now, by country, for the cap.
    let mut in_beds: BTreeMap<String, usize> = BTreeMap::new();
    for p in i.ward.open() {
        if let Some(c) = &p.country {
            *in_beds.entry(c.clone()).or_default() += 1;
        }
    }
    let cap = bed_cap(i.beds);
    let total_weight: f64 = i.pool.iter().map(|p| p.country.clone()).collect::<BTreeSet<_>>().iter().map(|c| i.weights.of(c)).sum();

    for slot in 0..i.want {
        // 1a. countries, by largest deficit against their share of everyone on the ward and
        // waiting; a country at the bed cap sits out; ties go to the country with fewest in beds,
        // then to the seed.
        let free: Vec<&Person> = i.pool.iter().filter(|p| !busy.contains(&p.key)).collect();
        let drawn_total: usize = load.country.values().sum();
        let mut countries: Vec<&str> = free.iter().map(|p| p.country.as_str()).collect::<BTreeSet<_>>().into_iter().collect();
        countries.retain(|c| in_beds.get(*c).copied().unwrap_or(0) < cap);
        let deficit = |c: &str| {
            let share = if total_weight > 0.0 { i.weights.of(c) / total_weight } else { 0.0 };
            share * (drawn_total as f64 + 1.0) - load.country(c) as f64
        };
        countries.sort_by(|a, b| {
            deficit(b)
                .partial_cmp(&deficit(a))
                .expect("finite")
                .then_with(|| in_beds.get(*a).copied().unwrap_or(0).cmp(&in_beds.get(*b).copied().unwrap_or(0)))
                .then_with(|| shuffle_key(i.seed, slot, a).cmp(&shuffle_key(i.seed, slot, b)))
        });

        // 2–4. the case for a person: one draw in five from her country's list when it has one,
        // otherwise the emptiest band with a case written for someone of her sex. Deterministic
        // per (seed, slot, person), so trying people in turn draws nothing twice.
        let choose = |who: &Person| -> Option<(&Case, bool)> {
            if let Some(list) = i.endemic.get(&who.country) {
                let draw = shuffle_key(i.seed, slot, &format!("{}/endemic", who.key)) % u64::from(ENDEMIC_IN);
                if draw == 0 {
                    let mut fits: Vec<&Case> = list.iter().filter_map(|id| i.catalogue.get(id)).filter(|c| c.sex == who.sex).collect();
                    fits.sort_by_key(|c| (load.case(&c.id), shuffle_key(i.seed, slot, &c.id)));
                    if let Some(c) = fits.first() {
                        return Some((c, true));
                    }
                }
            }
            let mut bands: Vec<&str> = BANDS.to_vec();
            bands.sort_by_key(|b| (load.band(b), shuffle_key(i.seed, slot, b)));
            for band in bands {
                let mut fits: Vec<&Case> = i.catalogue.in_band(band).into_iter().filter(|c| c.sex == who.sex).collect();
                fits.sort_by_key(|c| (load.case(&c.id), shuffle_key(i.seed, slot, &c.id)));
                if let Some(c) = fits.first() {
                    return Some((c, false));
                }
            }
            None
        };

        let mut placed = None;
        'countries: for country in countries {
            // Everyone free from this country, with the case each would get; a person for whom no
            // case fits is skipped, never forced. Within the country a face that already fits is
            // used before one is made, then the seed decides.
            let mut people: Vec<(&Person, &Case, bool, Option<crate::manifest::Base>)> = free
                .iter()
                .copied()
                .filter(|p| p.country == country)
                .filter_map(|p| choose(p).map(|(c, e)| (p, c, e, i.manifest.base_for(&p.key, &c.band))))
                .collect();
            people.sort_by_key(|(p, _, _, fit)| (fit.is_none(), shuffle_key(i.seed, slot, &p.key)));
            if let Some((who, case, endemic, fit)) = people.into_iter().next() {
                // 5. her age, inside the band and near her face if she has one that fits.
                let (base, age) = match fit {
                    Some(b) => {
                        let lo = (*case.band.start()).max(b.age.saturating_sub(NEAR_FACE));
                        let hi = (*case.band.end()).min(b.age.saturating_add(NEAR_FACE));
                        let age = lo + rng.below(u64::from(hi - lo) + 1) as u16;
                        (Base::Have { key: b.key, url: b.url, age: b.age }, age)
                    }
                    None => {
                        let (lo, hi) = (*case.band.start(), *case.band.end());
                        let age = lo + rng.below(u64::from(hi - lo) + 1) as u16;
                        (Base::Make { key: who.key.clone(), age }, age)
                    }
                };
                let portrait = match &base {
                    Base::Have { url, .. } => BTreeMap::from([("stable".to_string(), url.clone())]),
                    Base::Make { .. } => BTreeMap::new(),
                };
                placed = Some(Planned {
                    pack: Pack {
                        case: case.id.clone(),
                        persona: Persona { name: who.name.clone(), age, country: who.country.clone(), sex: who.sex.letter().into() },
                        portrait,
                        endemic,
                    },
                    person: who.key.clone(),
                    sex: who.sex,
                    place: who.place.clone(),
                    base,
                    weight: i.weights.of(&who.country),
                });
                break 'countries;
            }
        }

        match placed {
            Some(pl) => {
                busy.insert(pl.person.clone());
                load.count(&pl.pack.case, &pl.pack.persona.country);
                out.packs.push(pl);
            }
            None => {
                out.exhausted = true;
                out.notes.push(format!(
                    "pool exhausted: {} of {} people are free and none can take a case the factory can build \
                     (busy = on the ward or waiting for a bed); built {} of {} wanted",
                    free.len(),
                    i.pool.len(),
                    out.packs.len(),
                    i.want
                ));
                break;
            }
        }
    }
    out
}
