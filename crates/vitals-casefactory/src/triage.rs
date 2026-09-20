//! The ward's own early-warning read of a case's presenting vitals — NEWS2, on the same scale
//! and bands as the ward's `vitals-web/src/news2.rs` — used for one decision: a case whose
//! presenting vitals the score calls *low* with no single red parameter shows no deterioration
//! the shift can honestly stage, and is cut from the ward by name (the clinical advisor's ruling
//! 3.7, 20 Sep 2026). NEWS2 is an adult instrument; a child's presentation is left to the
//! paediatric shape's own gate.

use crate::embla::Vitals0;

/// NEWS2 is validated for adults only; the ward reads it from sixteen.
pub const ADULT_FROM_YEARS: u32 = 16;

fn resp(rr: f64) -> u32 {
    match rr {
        v if v <= 8.0 => 3,
        v if v <= 11.0 => 1,
        v if v <= 20.0 => 0,
        v if v <= 24.0 => 2,
        _ => 3,
    }
}

fn spo2(s: f64) -> u32 {
    match s {
        v if v <= 91.0 => 3,
        v if v <= 93.0 => 2,
        v if v <= 95.0 => 1,
        _ => 0,
    }
}

fn systolic(sbp: f64) -> u32 {
    match sbp {
        v if v <= 90.0 => 3,
        v if v <= 100.0 => 2,
        v if v <= 110.0 => 1,
        v if v <= 219.0 => 0,
        _ => 3,
    }
}

fn pulse(hr: f64) -> u32 {
    match hr {
        v if v <= 40.0 => 3,
        v if v <= 50.0 => 1,
        v if v <= 90.0 => 0,
        v if v <= 110.0 => 1,
        v if v <= 130.0 => 2,
        _ => 3,
    }
}

fn temperature(t: f64) -> u32 {
    match t {
        v if v <= 35.0 => 3,
        v if v < 36.1 => 1,
        v if v <= 38.0 => 0,
        v if v <= 39.0 => 1,
        _ => 2,
    }
}

fn consciousness(gcs: u8) -> u32 {
    if gcs >= 15 { 0 } else { 3 }
}

/// The aggregate and the worst single parameter, for an adult; `None` for a child, whom this
/// instrument does not read. Supplemental oxygen is not known at the door and scores nothing.
pub fn news2(v: &Vitals0, age: Option<u32>) -> Option<(u32, u32)> {
    if age.is_some_and(|a| a < ADULT_FROM_YEARS) {
        return None;
    }
    let parts = [resp(v.rr), spo2(v.spo2), systolic(v.sbp), pulse(v.hr), temperature(v.temp), consciousness(v.gcs)];
    Some((parts.iter().sum(), parts.iter().copied().max().unwrap_or(0)))
}

/// The ward's *low* band: an aggregate of four or under with nothing at three — routine
/// monitoring, no urgent response. `Some((total, worst))` when the presentation is low.
pub fn news2_low(v: &Vitals0, age: Option<u32>) -> Option<(u32, u32)> {
    news2(v, age).filter(|(total, worst)| *total <= 4 && *worst < 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(sbp: f64, hr: f64, rr: f64, spo2: f64, temp: f64, gcs: u8) -> Vitals0 {
        Vitals0 { sbp, dbp: sbp - 40.0, hr, rr, spo2, temp, gcs, assumed: vec![] }
    }

    #[test]
    fn the_bands_are_the_wards() {
        assert_eq!(news2(&v(120.0, 80.0, 16.0, 97.0, 37.0, 15), Some(40)), Some((0, 0)));
        assert_eq!(news2(&v(104.0, 108.0, 18.0, 97.0, 37.0, 15), Some(40)), Some((2, 1)));
        assert_eq!(news2(&v(88.0, 80.0, 18.0, 97.0, 37.0, 15), Some(40)), Some((3, 3)));
        assert_eq!(news2(&v(82.0, 130.0, 28.0, 94.0, 38.9, 14), Some(41)), Some((3 + 1 + 3 + 2 + 1 + 3, 3)));
        assert!(news2_low(&v(104.0, 108.0, 18.0, 97.0, 37.0, 15), Some(40)).is_some());
        assert!(news2_low(&v(88.0, 80.0, 18.0, 97.0, 37.0, 15), Some(40)).is_none(), "one red parameter is not low");
        assert!(news2_low(&v(110.0, 100.0, 22.0, 95.0, 38.5, 15), Some(40)).is_none(), "five is medium");
        assert!(news2(&v(104.0, 108.0, 18.0, 97.0, 37.0, 15), Some(8)).is_none(), "a child is not read");
        assert!(news2(&v(104.0, 108.0, 18.0, 97.0, 37.0, 15), None).is_some(), "no age is an adult");
    }
}
