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
//!      [`crate::need`] — by largest deficit against its share of everything this factory has
//!      sent (the ledger's chronology, then this plan's packs), so twice the need is twice the
//!      patients, exactly, over a run; and "least on the ward first" breaks ties within the
//!      weighting, never overrides it.
//!   3. **and from the whole world.** Founder, 16 Sep 2026: "ควรมีคนไข้จากทั่วโลกนะ". Need decides
//!      how often a country appears; the spread decides what the board and the queue look like
//!      at any one moment. The queue the ward admits from — read as the last [`QUEUE_WINDOW`]
//!      packs sent, the ledger's own chronology and then this plan's — holds no country more
//!      than [`QUEUE_CAP`] times, at least [`MIN_COUNTRIES`] countries and at least
//!      [`MIN_REGIONS`] of the ten regions in [`crate::region`]; and no region waits more than
//!      [`WORLD_WINDOW`] draws. The board never holds a country twice while another country is
//!      available (the 40 % bed cap, which with three beds is one). When need draws a country
//!      one of these will not take, the draw is done again over the countries still allowed —
//!      the same order, so the weights are renormalised over what is left — never a tick
//!      skipped, and the plan records which countries were redrawn and why. The queue cap is
//!      the one rule that yields to nothing: a queue that could only be filled with a third of
//!      one country stays short, and the plan says so.
//!   4. **her disease is not her origin.** Four draws in five ignore where she is from. One in
//!      [`vitals_web::ward::ENDEMIC_IN`], for a country with an endemic list, takes a case from
//!      that list — and only then is the pack `endemic`.
//!   5. **the bands are balanced** against what is in beds and what is waiting: the level with
//!      fewest patients is filled first, and within it the case least used.
//!   6. **her sex is the case's**, checked before the case is chosen rather than after; a person
//!      for whom no case fits is skipped, never forced.
//!   7. **her age is inside the case's band** — the door's band — and near her face's age when a
//!      face already fits, so the picture and the number agree. When no face fits, one is to be
//!      made at the age drawn, and the pack goes out without a picture rather than with a wrong
//!      one.

use crate::catalogue::{Case, Catalogue, Sex};
use crate::door::WardView;
use crate::ledger::Ledger;
use crate::manifest::Manifest;
use crate::need::Weights;
use crate::pool::Person;
use crate::region::{region_of, Region, ALL};
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

/// The queue the spread is read over: the last this-many packs sent, which is the queue the ward
/// admits from when the factory keeps it at its depth.
pub const QUEUE_WINDOW: usize = 20;
/// The most packs one country may hold among the last [`QUEUE_WINDOW`].
pub const QUEUE_CAP: usize = 2;
/// The fewest distinct countries a full queue must show.
pub const MIN_COUNTRIES: usize = 12;
/// The fewest regions a full queue must show.
pub const MIN_REGIONS: usize = 6;
/// No region goes this many draws without a patient.
pub const WORLD_WINDOW: usize = 40;

/// One soft rule of the spread: its name, who it keeps, and why it refuses the rest.
type SoftRule<'a> = (&'static str, Box<dyn Fn(&str) -> bool + 'a>, String);

/// One draw the spread would not take, done again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redraw {
    /// Which pack of this plan, from 0.
    pub slot: usize,
    /// The country need drew.
    pub drawn: String,
    /// Which rule refused it.
    pub why: String,
    /// The country the redraw took.
    pub took: String,
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
    /// Where in the world the country is; `None` for a code the region table does not place.
    pub region: Option<Region>,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub packs: Vec<Planned>,
    pub notes: Vec<String>,
    /// True when a pack was wanted and nobody was left to be it — or nobody the spread allows.
    pub exhausted: bool,
    /// Every draw the spread refused and did again, in slot order.
    pub redrawn: Vec<Redraw>,
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

/// What is on the ward or waiting, tallied by band and by case — what the bands are balanced
/// against. (Countries are tallied against the run, in `plan()`, not against the ward.)
#[derive(Default)]
struct Load {
    band: BTreeMap<&'static str, usize>,
    case: BTreeMap<String, usize>,
}

impl Load {
    fn count(&mut self, case: &str) {
        if let Some(b) = difficulty_of(case) {
            *self.band.entry(b).or_default() += 1;
        }
        *self.case.entry(case.to_string()).or_default() += 1;
    }
    fn band(&self, b: &str) -> usize {
        self.band.get(b).copied().unwrap_or(0)
    }
    fn case(&self, c: &str) -> usize {
        self.case.get(c).copied().unwrap_or(0)
    }
}

pub fn plan(i: &Inputs) -> Plan {
    let mut out = Plan::default();
    let mut rng = Rng::new(i.seed);

    // What is already on the ward or waiting, by person, band and case.
    let mut busy: BTreeSet<String> = i.ledger.busy_keys(i.ward, i.pool);
    let mut load = Load::default();
    for p in i.ward.open() {
        if let Some(case) = &p.case {
            load.count(case);
        }
    }
    for (_, s) in i.ledger.unseen() {
        load.count(&s.case);
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

    // The draw's history, oldest first: what the ledger sent, then what this plan builds. Need is
    // measured against it — the run, not the queue's snapshot — and the spread rules read its
    // tail. Measured against the ward and the queue alone, a country whose share of twenty slots
    // rounds to none would never be drawn once the queue was full; measured against the run,
    // twice the need is twice the patients over time, exactly.
    let mut sequence: Vec<String> = i.ledger.chronology().into_iter().map(|s| s.country.clone()).collect();
    let mut sent: BTreeMap<String, usize> = BTreeMap::new();
    for c in &sequence {
        *sent.entry(c.clone()).or_default() += 1;
    }
    let mut relaxed: BTreeSet<String> = BTreeSet::new();

    for slot in 0..i.want {
        // 1a. countries, by largest deficit against their share of everything sent so far; ties
        // go to the country with fewest in beds, then to the seed.
        let free: Vec<&Person> = i.pool.iter().filter(|p| !busy.contains(&p.key)).collect();
        let drawn_total = sequence.len();
        let mut countries: Vec<&str> = free.iter().map(|p| p.country.as_str()).collect::<BTreeSet<_>>().into_iter().collect();
        let deficit = |c: &str| {
            let share = if total_weight > 0.0 { i.weights.of(c) / total_weight } else { 0.0 };
            share * (drawn_total as f64 + 1.0) - sent.get(c).copied().unwrap_or(0) as f64
        };
        countries.sort_by(|a, b| {
            deficit(b)
                .partial_cmp(&deficit(a))
                .expect("finite")
                .then_with(|| in_beds.get(*a).copied().unwrap_or(0).cmp(&in_beds.get(*b).copied().unwrap_or(0)))
                .then_with(|| shuffle_key(i.seed, slot, a).cmp(&shuffle_key(i.seed, slot, b)))
        });
        let need_drew: Option<String> = countries.first().map(|c| c.to_string());

        // 2a. the spread. The queue is the last QUEUE_WINDOW of the sequence with this pack as
        // its newest; the world window the last WORLD_WINDOW. Each rule is a filter over the
        // need-ordered list, so the first country left is the redraw over what is still allowed.
        let queue_fixed: Vec<String> = tail(&sequence, QUEUE_WINDOW - 1).to_vec();
        let world_fixed: Vec<String> = tail(&sequence, WORLD_WINDOW - 1).to_vec();
        let queue_slots_left = QUEUE_WINDOW - queue_fixed.len();
        let world_slots_left = WORLD_WINDOW - world_fixed.len();
        let count_in = |window: &[String], c: &str| window.iter().filter(|x| x.as_str() == c).count();
        let regions_in = |window: &[String]| -> BTreeSet<Region> { window.iter().filter_map(|c| region_of(c)).collect() };
        let queue_countries: BTreeSet<&str> = queue_fixed.iter().map(String::as_str).collect();
        let queue_regions = regions_in(&queue_fixed);
        let world_regions = regions_in(&world_fixed);
        let world_absent: BTreeSet<Region> = ALL.iter().copied().filter(|r| !world_regions.contains(r)).collect();

        let mut refused: BTreeMap<String, String> = BTreeMap::new();
        // The cap yields to nothing.
        countries.retain(|c| {
            let n = count_in(&queue_fixed, c);
            if n >= QUEUE_CAP {
                refused.insert(c.to_string(), format!("at the queue cap: {n} of the last {QUEUE_WINDOW}"));
                false
            } else {
                true
            }
        });
        // The rest yield when nobody else is available, and the plan says so once per rule.
        let mut rules: Vec<SoftRule> = vec![(
            "the board rule",
            Box::new(|c| in_beds.get(c).copied().unwrap_or(0) < cap),
            format!("in a bed, and the board holds a country at most {cap} of {} beds", i.beds),
        )];
        if world_absent.len() >= world_slots_left {
            rules.push(("the world rule", Box::new(|c| region_of(c).is_some_and(|r| world_absent.contains(&r))), format!("a region has waited {} draws and the next must be from it", WORLD_WINDOW - 1)));
        }
        if queue_countries.len() + queue_slots_left <= MIN_COUNTRIES {
            rules.push(("the twelve-countries rule", Box::new(|c| !queue_countries.contains(c)), format!("the queue must reach {MIN_COUNTRIES} countries and the next must be new")));
        }
        if queue_regions.len() + queue_slots_left <= MIN_REGIONS {
            rules.push(("the six-regions rule", Box::new(|c| region_of(c).is_some_and(|r| !queue_regions.contains(&r))), format!("the queue must reach {MIN_REGIONS} regions and the next must be from a new one")));
        }
        for (rule, keep, why) in &rules {
            if !narrow(&mut countries, keep, why, &mut refused) && !countries.is_empty() && relaxed.insert(rule.to_string()) {
                out.notes.push(format!("{rule}: no other country is available, so the rule yields for this tick"));
            }
        }

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
                    region: region_of(&who.country),
                });
                break 'countries;
            }
        }

        match placed {
            Some(pl) => {
                if let Some(drawn) = need_drew.filter(|d| *d != pl.pack.persona.country) {
                    let why = refused.get(&drawn).cloned().unwrap_or_else(|| "nobody free from it fits a case the factory can build".to_string());
                    out.redrawn.push(Redraw { slot, drawn, why, took: pl.pack.persona.country.clone() });
                }
                busy.insert(pl.person.clone());
                load.count(&pl.pack.case);
                sequence.push(pl.pack.persona.country.clone());
                *sent.entry(pl.pack.persona.country.clone()).or_default() += 1;
                out.packs.push(pl);
            }
            None if free.is_empty() || refused.is_empty() => {
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
            None => {
                out.exhausted = true;
                let capped: Vec<String> = refused.iter().map(|(c, why)| format!("{c} ({why})")).collect();
                out.notes.push(format!(
                    "queue cap: every free country the spread would take is refused — {}; built {} of {} wanted, and the queue stays short rather than hold one country three times",
                    capped.join(", "),
                    out.packs.len(),
                    i.want
                ));
                break;
            }
        }
    }
    if !out.redrawn.is_empty() {
        let mut per: BTreeMap<(&str, &str), usize> = BTreeMap::new();
        for r in &out.redrawn {
            *per.entry((r.drawn.as_str(), r.why.as_str())).or_default() += 1;
        }
        let cells: Vec<String> = per.iter().map(|((c, why), n)| format!("{c} ×{n} — {why}")).collect();
        out.notes.push(format!("redrawn {} of {} draws: {}", out.redrawn.len(), out.packs.len(), cells.join("; ")));
    }
    out
}

/// The last `n` of a sequence, or all of it when it is shorter.
fn tail(seq: &[String], n: usize) -> &[String] {
    &seq[seq.len().saturating_sub(n)..]
}

/// One soft rule of the spread: keep the countries `keep` allows, recording `why` against the
/// rest, when any is left — and keep them all, returning false, when the rule would leave nobody.
fn narrow(countries: &mut Vec<&str>, keep: &dyn Fn(&str) -> bool, why: &str, refused: &mut BTreeMap<String, String>) -> bool {
    if !countries.iter().any(|c| keep(c)) {
        return false;
    }
    countries.retain(|c| {
        if keep(c) {
            true
        } else {
            refused.entry(c.to_string()).or_insert_with(|| why.to_string());
            false
        }
    });
    true
}
