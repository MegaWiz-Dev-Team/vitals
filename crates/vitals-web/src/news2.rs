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

/// One set of observations, as a nurse would record them.
#[derive(Debug, Clone, Copy)]
pub struct Obs {
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

pub fn score(o: &Obs) -> Score {
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
    Score { total, worst, band: band_for(total, worst) }
}
