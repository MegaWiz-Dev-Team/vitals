//! Which case a patient presents: chosen from the ward's own list.
//!
//! The ward's case door (`GET /api/ward/cases`, 7b, 16 Sep 2026) lists every case it holds, and
//! its queue door reads an optional `case_id` and `difficulty` on a pack. The factory chooses one
//! for every pack it builds or re-sends, by these rules, in this order:
//!
//!   1. **an endemic case of her country** when the list has one and her age and sex fit its
//!      band — the band the factory knows from its own catalogue, never a guess;
//!   2. else **any case with no country**, at the difficulty the queue is short of, so student,
//!      intern and resident stay about 1:1:1 across the board and the queue (the founder's rule:
//!      difficulty levels exist so a stranger can choose); among those, the one least often in
//!      the queue already, then her own case (the one her age was drawn inside), then the seed;
//!   3. **never** an endemic case of another country; **never** a case a bed holds (the board says
//!      `case` today, `case_id` once 7b's build lands; either is read); **never** a case whose
//!      band the factory does not know — the four episodes and any community case without a
//!      persona file — because "her age fits" is a claim the factory can only make about a case
//!      it has.
//!
//! When nothing fits, the choice is `None` and the pack goes out without a case_id rather than
//! with a wrong one. The ward's default applies, and the tick says so.

use crate::catalogue::{Catalogue, Sex};
use crate::door::WardCase;
use std::collections::{BTreeMap, BTreeSet};

/// Who the case is for: her country, sex, the age drawn, and the case that age was drawn inside.
#[derive(Debug, Clone, Copy)]
pub struct Patient<'a> {
    pub country: &'a str,
    pub sex: Sex,
    pub age: u16,
    pub own_case: &'a str,
}

/// What the queue and the board already hold, by difficulty and by case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mix {
    difficulty: BTreeMap<String, usize>,
    case: BTreeMap<String, usize>,
}

impl Mix {
    /// One patient, on the board or waiting, at this difficulty on this case.
    pub fn count(&mut self, difficulty: &str, case_id: &str) {
        *self.difficulty.entry(difficulty.to_string()).or_default() += 1;
        *self.case.entry(case_id.to_string()).or_default() += 1;
    }
    /// How many at this difficulty.
    pub fn of(&self, difficulty: &str) -> usize {
        self.difficulty.get(difficulty).copied().unwrap_or(0)
    }
    /// How many on this case.
    pub fn queued(&self, case_id: &str) -> usize {
        self.case.get(case_id).copied().unwrap_or(0)
    }
}

/// The case chosen, its difficulty as the ward lists it, and why it was this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub case_id: String,
    pub difficulty: String,
    pub why: String,
}

/// A stable per-key shuffle value for one seed and one slot — the same construction plan.rs uses.
fn shuffle(seed: u64, slot: usize, key: &str) -> u64 {
    let mut x = (seed ^ (slot as u64).wrapping_mul(0xA24B_AED4_963E_E407)).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for b in key.bytes() {
        x ^= u64::from(b);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
    }
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// Choose her case from the ward's list. `in_beds` are the cases the board holds; `mix` is what
/// the board and the queue hold by difficulty and by case.
pub fn choose(cases: &[WardCase], catalogue: &Catalogue, who: &Patient, in_beds: &BTreeSet<String>, mix: &Mix, seed: u64, slot: usize) -> Option<Chosen> {
    // Does she fit the case as the factory knows it? Unknown is not a fit.
    let fits = |case_id: &str| catalogue.get(case_id).is_some_and(|c| c.sex == who.sex && c.band.contains(&who.age));
    let free = |w: &WardCase| !in_beds.contains(&w.case_id) && fits(&w.case_id);

    // 1. an endemic case of her country.
    let mut endemic: Vec<&WardCase> = cases.iter().filter(|w| w.endemic && w.country.as_deref() == Some(who.country) && free(w)).collect();
    endemic.sort_by_key(|w| (mix.queued(&w.case_id), shuffle(seed, slot, &w.case_id)));
    if let Some(w) = endemic.first() {
        return Some(Chosen { case_id: w.case_id.clone(), difficulty: w.difficulty.clone(), why: format!("endemic in {}", who.country) });
    }

    // 2. any case of the common draw, at the difficulty the queue is short of.
    let mut common: Vec<&WardCase> = cases.iter().filter(|w| !w.endemic && w.country.is_none() && free(w)).collect();
    common.sort_by_key(|w| (mix.of(&w.difficulty), mix.queued(&w.case_id), w.case_id != who.own_case, shuffle(seed, slot, &w.case_id)));
    let w = common.first()?;
    let why = if w.case_id == who.own_case {
        format!("own case, {} least filled", w.difficulty)
    } else {
        format!("{} least filled in the queue, {} times queued", w.difficulty, mix.queued(&w.case_id))
    };
    Some(Chosen { case_id: w.case_id.clone(), difficulty: w.difficulty.clone(), why })
}
