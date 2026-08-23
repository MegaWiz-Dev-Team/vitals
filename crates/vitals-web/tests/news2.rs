//! NEWS2, from the published table.
//!
//! The panel used to show a "stability" percentage invented for this app: a weighted blend of
//! four vitals against normal ranges. It read like a health bar because that is what it was, and
//! nothing on a real ward shows it.
//!
//! NEWS2 is what a real ward shows. It is the Royal College of Physicians' National Early Warning
//! Score 2 — an aggregate of seven observations, published and unencumbered, and the thing a
//! nurse escalates on. Every boundary below is taken from that table, so these tests are the
//! specification rather than a restatement of the implementation.

use vitals_web::news2 as news;

/// Respiration rate: ≤8 → 3, 9–11 → 1, 12–20 → 0, 21–24 → 2, ≥25 → 3
#[test]
fn respiration_rate_scores_at_the_published_boundaries() {
    for (rr, want) in [(8.0, 3), (9.0, 1), (11.0, 1), (12.0, 0), (20.0, 0), (21.0, 2), (24.0, 2), (25.0, 3)] {
        assert_eq!(news::resp(rr), want, "rr {rr}");
    }
}

/// SpO2 scale 1: ≤91 → 3, 92–93 → 2, 94–95 → 1, ≥96 → 0
#[test]
fn oxygen_saturation_scores_at_the_published_boundaries() {
    for (s, want) in [(91.0, 3), (92.0, 2), (93.0, 2), (94.0, 1), (95.0, 1), (96.0, 0), (100.0, 0)] {
        assert_eq!(news::spo2(s), want, "spo2 {s}");
    }
}

/// Systolic: ≤90 → 3, 91–100 → 2, 101–110 → 1, 111–219 → 0, ≥220 → 3
///
/// The top of the range scores as heavily as the bottom, which a linear "distance from normal"
/// meter cannot express and this one must.
#[test]
fn systolic_pressure_scores_high_at_both_ends() {
    for (s, want) in [(90.0, 3), (91.0, 2), (100.0, 2), (101.0, 1), (110.0, 1), (111.0, 0), (219.0, 0), (220.0, 3)] {
        assert_eq!(news::systolic(s), want, "sbp {s}");
    }
}

/// Pulse: ≤40 → 3, 41–50 → 1, 51–90 → 0, 91–110 → 1, 111–130 → 2, ≥131 → 3
#[test]
fn pulse_scores_at_the_published_boundaries() {
    for (p, want) in [(40.0, 3), (41.0, 1), (50.0, 1), (51.0, 0), (90.0, 0), (91.0, 1), (110.0, 1), (111.0, 2), (130.0, 2), (131.0, 3)] {
        assert_eq!(news::pulse(p), want, "hr {p}");
    }
}

/// Temperature: ≤35.0 → 3, 35.1–36.0 → 1, 36.1–38.0 → 0, 38.1–39.0 → 1, ≥39.1 → 2
#[test]
fn temperature_scores_at_the_published_boundaries() {
    for (t, want) in [(35.0, 3), (35.1, 1), (36.0, 1), (36.1, 0), (38.0, 0), (38.1, 1), (39.0, 1), (39.1, 2)] {
        assert_eq!(news::temperature(t), want, "temp {t}");
    }
}

/// Consciousness: alert → 0, anything less (CVPU) → 3. GCS 15 is alert.
#[test]
fn anything_but_alert_scores_three() {
    assert_eq!(news::consciousness(15), 0);
    assert_eq!(news::consciousness(14), 3);
    assert_eq!(news::consciousness(3), 3);
}

/// Supplemental oxygen is itself worth 2 — a patient held at 96% on a mask is sicker than one
/// holding 96% on air, and the score has to say so.
#[test]
fn being_on_oxygen_costs_two() {
    assert_eq!(news::supplemental(false), 0);
    assert_eq!(news::supplemental(true), 2);
}

#[test]
fn a_well_adult_on_air_scores_zero() {
    let s = news::score(&news::Obs { rr: 16.0, spo2: 98.0, on_oxygen: false, sbp: 120.0, hr: 70.0, temp: 36.8, gcs: 15 });
    assert_eq!(s.total, 0);
    assert_eq!(s.band, news::Band::Low);
}

/// EP1's patient shortly after the sting: tachycardic, hypotensive, hypoxic, on oxygen.
#[test]
fn a_patient_in_anaphylaxis_scores_high_and_says_so() {
    let s = news::score(&news::Obs { rr: 28.0, spo2: 90.0, on_oxygen: true, sbp: 86.0, hr: 128.0, temp: 36.9, gcs: 15 });
    // 3 (rr) + 3 (spo2) + 2 (oxygen) + 3 (sbp) + 2 (hr) + 0 + 0
    assert_eq!(s.total, 13);
    assert_eq!(s.band, news::Band::High);
}

/// A single 3 in any one observation escalates on its own, even when the total looks calm. That
/// rule exists because one catastrophic derangement is not averaged away by six normal ones, and
/// averaging is exactly what the old percentage did.
#[test]
fn one_extreme_observation_escalates_on_its_own() {
    let s = news::score(&news::Obs { rr: 16.0, spo2: 98.0, on_oxygen: false, sbp: 88.0, hr: 70.0, temp: 36.8, gcs: 15 });
    assert_eq!(s.total, 3);
    assert_eq!(s.worst, 3, "the systolic alone is a 3");
    assert_eq!(s.band, news::Band::Medium, "a single 3 is not a low-risk patient");
}

#[test]
fn the_bands_follow_the_published_thresholds() {
    let at = |total: u32| news::band_for(total, 1);
    assert_eq!(at(0), news::Band::Low);
    assert_eq!(at(4), news::Band::Low);
    assert_eq!(at(5), news::Band::Medium);
    assert_eq!(at(6), news::Band::Medium);
    assert_eq!(at(7), news::Band::High);
    assert_eq!(at(20), news::Band::High);
}

/// Nothing may fall off the end of the table.
#[test]
fn absurd_readings_still_score() {
    let s = news::score(&news::Obs { rr: 0.0, spo2: 0.0, on_oxygen: true, sbp: 0.0, hr: 0.0, temp: -50.0, gcs: 3 });
    assert_eq!(s.total, 3 + 3 + 2 + 3 + 3 + 3 + 3);
    let s = news::score(&news::Obs { rr: 999.0, spo2: 200.0, on_oxygen: false, sbp: 999.0, hr: 999.0, temp: 99.0, gcs: 15 });
    assert_eq!(s.total, 3 + 0 + 0 + 3 + 3 + 2 + 0);
}
