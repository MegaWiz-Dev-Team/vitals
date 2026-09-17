//! The ward's list is the catalogue: which case a patient presents, and who fits it.
//!
//! Coordinator, 16 Sep 2026, after a dry run against staging: the queue door refuses season ids,
//! so the factory carries no case list of its own. A pack's `case` is a World case_id from
//! `GET /api/ward/cases`, and the case is chosen first, then a person of the case's sex.
//!
//! **Fit** ([`fits`]). A row states its patient — `patient: {age, sex}`, the sex spelled `male` /
//! `female` (ward commit 92b4181) — or states nobody. Her sex must match the patient's; her age
//! is within [`AGE_SLACK`] years of the patient's; a case under [`CHILD_UNDER`] takes a persona
//! under it only, and a case of that age or more a persona of that age or more. A row with no
//! patient fits any adult ([`ADULT_ANY`]) of either sex — never "no case".
//!
//! **Endemic** is the list's to say, not a file's: a country "has an endemic list" iff a placeable
//! case is tagged endemic for it ([`has_endemic_list`]). The founder's one draw in
//! [`ENDEMIC_IN`] for such a country ([`endemic_draw`]) takes that case — and only then does the
//! pack carry the tag, which is exactly the claim the ward's door checks (endemic, and written for
//! her country). The other four draws in five are the common draw, in which place plays no part.
//! When nothing of the common draw is placeable at all — staging on 17 Sep, revision 00034: every
//! placeable case endemic — the case written for home is the only case there is and is taken on
//! every draw; the reason says so. A withdrawn row is never chosen ([`placeable`]).
//!
//! **Order** ([`rank`]), for the country need drew:
//!
//!   1. on the endemic draw, the endemic cases written for her country, the least often queued
//!      first (the one draw in five is what keeps a queue of Thais from being a queue of dengues);
//!   2. then every case of the common draw — `country` null and not endemic, or written for her
//!      country and not endemic — at the level the board and the queue are short of, so student,
//!      intern and resident stay about 1:1:1 (the founder's rule: levels exist so a stranger can
//!      choose); within a level the case least often in the queue already, then the seed;
//!   3. never a case written for another country; never a case a bed holds (`patients[].case`,
//!      the World case id since the ward's 0543ed7; a bed's own `endemic` says the patient was
//!      drawn from her country's list and is not read); never a withdrawn row.
//!
//! The mix the level is balanced against is counted from what the factory knows — the list's
//! level for a bed's case, the ledger's own for the queue — never from the board's `difficulty`,
//! which is null for patients admitted before 0543ed7.

use crate::door::WardCase;
use crate::sex::Sex;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;
use vitals_web::ward::ENDEMIC_IN;

/// How far from the case's patient a persona's age may sit.
pub const AGE_SLACK: u16 = 12;
/// A case whose patient is younger than this takes a persona younger than this, and only such a
/// persona; a case of this age or more takes nobody younger.
pub const CHILD_UNDER: u16 = 16;
/// The ages a case with no stated patient fits: any adult.
pub const ADULT_ANY: RangeInclusive<u16> = 18..=85;

/// The pool's letter for the case door's word — `male` / `female`, or the letters themselves —
/// and `None` for a case that states no patient. The one place the word is read.
pub fn sex_of(case: &WardCase) -> Option<Sex> {
    let word = case.patient.as_ref()?.sex.trim().to_lowercase();
    match word.as_str() {
        "male" | "m" => Some(Sex::M),
        "female" | "f" => Some(Sex::F),
        _ => None,
    }
}

/// The ages a persona may be given for this case.
pub fn age_window(case: &WardCase) -> RangeInclusive<u16> {
    match &case.patient {
        None => ADULT_ANY,
        Some(p) if p.age < CHILD_UNDER => p.age.saturating_sub(AGE_SLACK).max(1)..=(p.age + AGE_SLACK).min(CHILD_UNDER - 1),
        Some(p) => p.age.saturating_sub(AGE_SLACK).max(CHILD_UNDER)..=p.age + AGE_SLACK,
    }
}

/// Does a persona of this sex at this age fit the case?
pub fn fits(case: &WardCase, sex: Sex, age: u16) -> bool {
    let sex_fits = match sex_of(case) {
        Some(theirs) => theirs == sex,
        // A stated patient whose sex is a word the factory does not read fits nobody; no stated
        // patient fits either sex.
        None => case.patient.is_none(),
    };
    sex_fits && age_window(case).contains(&age)
}

/// What the board and the queue already hold, by level and by case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mix {
    difficulty: BTreeMap<String, usize>,
    case: BTreeMap<String, usize>,
}

impl Mix {
    /// One patient, on the board or waiting, at this level on this case.
    pub fn count(&mut self, difficulty: &str, case_id: &str) {
        *self.difficulty.entry(difficulty.to_string()).or_default() += 1;
        *self.case.entry(case_id.to_string()).or_default() += 1;
    }
    /// How many at this level.
    pub fn of(&self, difficulty: &str) -> usize {
        self.difficulty.get(difficulty).copied().unwrap_or(0)
    }
    /// How many on this case.
    pub fn queued(&self, case_id: &str) -> usize {
        self.case.get(case_id).copied().unwrap_or(0)
    }
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

/// The rows the ward will place: everything not withdrawn.
pub fn placeable(cases: &[WardCase]) -> Vec<WardCase> {
    cases.iter().filter(|w| !w.withdrawn).cloned().collect()
}

/// Does this country have an endemic list — a placeable case tagged endemic and written for it?
pub fn has_endemic_list(cases: &[WardCase], country: &str) -> bool {
    cases.iter().any(|w| !w.withdrawn && w.endemic && w.country.as_deref() == Some(country))
}

/// The founder's one draw in [`ENDEMIC_IN`]: for a country with an endemic list, this slot's draw
/// takes a case from it. Deterministic per seed, slot and country; never for a country without.
pub fn endemic_draw(cases: &[WardCase], country: &str, seed: u64, slot: usize) -> bool {
    has_endemic_list(cases, country) && shuffle(seed, slot, &format!("{country}/endemic")).is_multiple_of(u64::from(ENDEMIC_IN))
}

/// The cases a patient from `country` may present, best first, each with why it sits where it
/// does. `in_beds` are the cases the board holds; `mix` is what the board and the queue hold;
/// `on_endemic_draw` is this slot's [`endemic_draw`] for the country.
pub fn rank<'a>(cases: &'a [WardCase], country: &str, in_beds: &BTreeSet<String>, mix: &Mix, seed: u64, slot: usize, on_endemic_draw: bool) -> Vec<(&'a WardCase, String)> {
    let free = |w: &WardCase| !w.withdrawn && !in_beds.contains(&w.case_id);
    let hers = |w: &WardCase| w.country.as_deref() == Some(country);
    // The common draw: cases of no country, and cases written for her country but not endemic.
    let mut common: Vec<&WardCase> = cases.iter().filter(|w| !w.endemic && (w.country.is_none() || hers(w)) && free(w)).collect();
    common.sort_by_key(|w| (mix.of(&w.difficulty), mix.queued(&w.case_id), shuffle(seed, slot, &w.case_id)));
    // The endemic cases written for her: on the endemic draw — or on every draw when nothing of
    // the common draw is placeable at all, since then they are the only cases there are. The one
    // draw in five is what keeps a queue of Thais from being a queue of dengues; within the list
    // the case least often queued comes first.
    let nothing_common = !cases.iter().any(|w| !w.endemic && w.country.is_none() && !w.withdrawn);
    let mut endemic: Vec<&WardCase> = if on_endemic_draw || nothing_common {
        cases.iter().filter(|w| w.endemic && hers(w) && free(w)).collect()
    } else {
        Vec::new()
    };
    endemic.sort_by_key(|w| (mix.queued(&w.case_id), shuffle(seed, slot, &w.case_id)));
    let mut out: Vec<(&WardCase, String)> = Vec::with_capacity(endemic.len() + common.len());
    for w in endemic {
        out.push((w, if on_endemic_draw { format!("endemic in {country}, on the one draw in {ENDEMIC_IN}") } else { format!("endemic in {country}, the only cases placeable") }));
    }
    for w in common {
        out.push((w, format!("{} least filled in the queue, {} times queued", w.difficulty, mix.queued(&w.case_id))));
    }
    out
}
