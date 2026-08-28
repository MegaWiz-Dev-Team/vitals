//! NEWS2 — the National Early Warning Score 2, as published by the Royal College of Physicians.
//!
//! What replaced a "stability" percentage this app invented: a weighted blend of four vitals
//! against normal ranges, rendered as a bar. It looked like a health bar because it was one, and
//! it was wrong in a way that mattered — averaging. A patient with one catastrophic derangement
//! and six normal observations came out looking well, which is the exact mistake NEWS2 exists to
//! prevent. The score a ward escalates on is not an average.
//!
//! Every boundary here is from the published table. The tests in `tests/news2.rs` are that table
//! written out, so they are the specification and this is the implementation of it.
//!
//! ## Who it is for
//!
//! **Adults.** The Royal College of Physicians says so in the publication itself: NEWS2 is not
//! validated in patients under 16, and the reason is not caution — it is that the thresholds are
//! wrong for a child. A well three-year-old breathes 28 a minute with a pulse of 118 and a
//! systolic of 98; put those through the table below and you get RR 3, HR 2, SBP 2, and a total
//! of 7 with "emergency response" beside it, on a child a paediatrician would walk past.
//!
//! This module had no idea how old anyone was, so it did exactly that on `osce-b3` — the bedside
//! monitor beside it correctly showing the 3–5 year bands, no alarm, MONITORING, and the patient
//! banner reading "Stable". Three instruments on one screen, one of them screaming.
//!
//! So [`Obs`] carries the age and [`score`] returns nothing for a child. **It does not invent a
//! paediatric score.** PEWS exists, it is not NEWS2, its charts are age-banded and locally
//! varied, and choosing one is a clinical decision that does not belong in a rendering path.
//! Showing nothing, and saying why, is the honest answer — and it is what [`NOT_VALIDATED`] is
//! for, because a blank box where a score used to be reads as reassurance and this is not that.

/// The youngest patient NEWS2 was validated for.
pub const ADULT_FROM_YEARS: f64 = 16.0;

/// What a screen says where the score would have gone.
///
/// A sentence rather than a blank, and a sentence that does not reassure: "no score" and an empty
/// panel both read as "nothing to worry about" on a child who may be very sick indeed. This says
/// which instrument is missing and why, and leaves the judgement where it belongs.
pub const NOT_VALIDATED: &str = "NEWS2 is not validated under 16";

/// May NEWS2 be reported for a patient of this age?
///
/// `None` — a case that declares no age — is treated as an adult, which is the published default
/// and what every screen here did before ages existed. That is only safe because the case table
/// declares an age for every case and a test fails if one is missing; without it, "no age" would
/// become the way a child gets scored as an adult again.
pub fn applies_to_age(age_years: Option<f64>) -> bool {
    age_years.is_none_or(|a| a >= ADULT_FROM_YEARS)
}

/// One set of observations, as a nurse would record them.
#[derive(Debug, Clone, Copy)]
pub struct Obs {
    /// How old the patient is, in years. Not an observation — it decides whether the observations
    /// may be scored at all. It sits in the same struct as the vitals precisely so that nobody
    /// can assemble a patient for scoring without having answered the question: the bug this
    /// field exists to end was `Obs` having no way to express a three-year-old.
    pub age_years: Option<f64>,
    pub rr: f64,
    pub spo2: f64,
    /// Whether the patient is on supplemental oxygen at all. Worth points on its own.
    pub on_oxygen: bool,
    pub sbp: f64,
    pub hr: f64,
    pub temp: f64,
    /// Consciousness, via GCS. NEWS2 asks only alert-or-not.
    pub gcs: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Low,
    Medium,
    High,
}

impl Band {
    pub fn as_str(self) -> &'static str {
        match self {
            Band::Low => "low",
            Band::Medium => "medium",
            Band::High => "high",
        }
    }
    /// What the score asks you to do about it.
    pub fn response(self) -> &'static str {
        match self {
            Band::Low => "routine observations",
            Band::Medium => "urgent review",
            Band::High => "emergency response",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Score {
    pub total: u32,
    /// The single worst observation. A 3 anywhere escalates on its own.
    pub worst: u32,
    pub band: Band,
}

pub fn resp(rr: f64) -> u32 {
    match rr {
        v if v <= 8.0 => 3,
        v if v <= 11.0 => 1,
        v if v <= 20.0 => 0,
        v if v <= 24.0 => 2,
        _ => 3,
    }
}

pub fn spo2(s: f64) -> u32 {
    match s {
        v if v <= 91.0 => 3,
        v if v <= 93.0 => 2,
        v if v <= 95.0 => 1,
        _ => 0,
    }
}

pub fn supplemental(on: bool) -> u32 {
    if on {
        2
    } else {
        0
    }
}

/// Scores at both ends. A linear distance-from-normal meter cannot express this, which is one
/// reason the old percentage could not be repaired.
pub fn systolic(sbp: f64) -> u32 {
    match sbp {
        v if v <= 90.0 => 3,
        v if v <= 100.0 => 2,
        v if v <= 110.0 => 1,
        v if v <= 219.0 => 0,
        _ => 3,
    }
}

pub fn pulse(hr: f64) -> u32 {
    match hr {
        v if v <= 40.0 => 3,
        v if v <= 50.0 => 1,
        v if v <= 90.0 => 0,
        v if v <= 110.0 => 1,
        v if v <= 130.0 => 2,
        _ => 3,
    }
}

pub fn temperature(t: f64) -> u32 {
    match t {
        v if v <= 35.0 => 3,
        v if v < 36.1 => 1,
        v if v <= 38.0 => 0,
        v if v <= 39.0 => 1,
        _ => 2,
    }
}

/// Alert, or not. NEWS2 does not grade the levels below alert — new confusion scores the same as
/// unresponsive, because both mean the same thing about how quickly someone must come.
pub fn consciousness(gcs: u8) -> u32 {
    if gcs >= 15 {
        0
    } else {
        3
    }
}

pub fn band_for(total: u32, worst: u32) -> Band {
    if total >= 7 {
        Band::High
    } else if total >= 5 || worst >= 3 {
        Band::Medium
    } else {
        Band::Low
    }
}

/// The score, or nothing at all for a patient NEWS2 does not cover.
///
/// `None` is not "zero" and not "unknown": it is "this instrument does not read this patient".
/// See [`NOT_VALIDATED`] for what to put on the screen in its place.
pub fn score(o: &Obs) -> Option<Score> {
    if !applies_to_age(o.age_years) {
        return None;
    }
    let parts = [
        resp(o.rr),
        spo2(o.spo2),
        supplemental(o.on_oxygen),
        systolic(o.sbp),
        pulse(o.hr),
        temperature(o.temp),
        consciousness(o.gcs),
    ];
    let total: u32 = parts.iter().sum();
    // Supplemental oxygen is a 2 by definition and never an escalating 3, so it cannot be the
    // worst single observation — but taking the max over everything is simpler than special
    // cases and reaches the same answer.
    let worst = parts.iter().copied().max().unwrap_or(0);
    Some(Score { total, worst, band: band_for(total, worst) })
}
