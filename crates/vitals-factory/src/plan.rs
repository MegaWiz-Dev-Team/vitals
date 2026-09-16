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
//!      ([`crate::ledger::Ledger::busy_keys`]). Countries with fewer people on the ward come
//!      first, so the globe fills before it repeats.
//!   2. **her disease is not her origin.** Four draws in five ignore where she is from. One in
//!      [`vitals_web::ward::ENDEMIC_IN`], for a country with an endemic list, takes a case from
//!      that list — and only then is the pack `endemic`.
//!   3. **the bands are balanced** against what is in beds and what is waiting: the level with
//!      fewest patients is filled first, and within it the case least used.
//!   4. **her sex is the case's**, checked before the case is chosen rather than after; a person
//!      for whom no case fits is skipped, never forced.
//!   5. **her age is inside the case's band** — the door's band — and near her face's age when a
//!      face already fits, so the picture and the number agree. When no face fits, one is to be
//!      made at the age drawn, and the pack goes out without a picture rather than with a wrong
//!      one.

use crate::catalogue::{Case, Catalogue, Sex};
use crate::door::WardView;
use crate::ledger::Ledger;
use crate::manifest::Manifest;
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
    /// How many packs to build.
    pub want: usize,
    pub seed: u64,
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

    for slot in 0..i.want {
        // 1. the people who are free, countries least on the ward first.
        let mut free: Vec<&Person> = i.pool.iter().filter(|p| !busy.contains(&p.key)).collect();
        free.sort_by_key(|p| (load.country(&p.country), shuffle_key(i.seed, slot, &p.key)));

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

        // A face that already fits is used before one is made: the first pass takes only people
        // whose face fits the case they would get, the second takes anyone with a case.
        let mut placed = None;
        'passes: for need_face in [true, false] {
            for who in &free {
                let Some((case, endemic)) = choose(who) else { continue };
                let fit = i.manifest.base_for(&who.key, &case.band);
                if need_face && fit.is_none() {
                    continue;
                }
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
                });
                break 'passes;
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
