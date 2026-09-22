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

/// The age every case in this file is about. The published table is an adult table; the rows
/// below are that table, so every patient in them is an adult and says so.
const ADULT: Option<f64> = Some(40.0);

// ── who the table is for ────────────────────────────────────────────────────
//
// The Royal College of Physicians publishes NEWS2 as an adult score and says it is not validated
// under 16. It is not a caution: the thresholds are wrong for a child, and applying them to one
// produces a confident number that is nonsense in the direction of alarm.

/// A well three-year-old, from `osce-b3`: HR 118, RR 28, SpO2 98 on air, BP 98/62. Every one of
/// those is normal for three. The adult table charges her 3 + 2 + 2 and calls for an emergency
/// response, on a child the bedside monitor beside it correctly reports as MONITORING.
#[test]
fn a_three_year_old_gets_no_score_rather_than_an_adults() {
    let child = news::Obs {
        age_years: Some(3.0),
        rr: 28.0, spo2: 98.0, on_oxygen: false, sbp: 98.0, hr: 118.0, temp: 37.2, gcs: 15,
    };
    assert!(news::score(&child).is_none(), "a three-year-old was scored on the adult table");

    // …and this is what the adult table would have said about her, which is the whole problem.
    let as_adult = news::Obs { age_years: ADULT, ..child };
    let s = news::score(&as_adult).expect("the adult table still works");
    assert_eq!(s.total, 7, "the numbers this station used to print");
    assert_eq!(s.band, news::Band::High);
}

/// Sixteen is the boundary the publication draws, and it is drawn on the sixteenth birthday.
#[test]
fn sixteen_is_scored_and_the_day_before_is_not() {
    let at = |years: f64| {
        news::score(&news::Obs {
            age_years: Some(years),
            rr: 16.0, spo2: 98.0, on_oxygen: false, sbp: 120.0, hr: 70.0, temp: 36.8, gcs: 15,
        })
    };
    assert_eq!(news::ADULT_FROM_YEARS, 16.0);
    assert!(at(16.0).is_some(), "sixteen is an adult for this score");
    assert!(at(15.9).is_none(), "under sixteen was scored");
    assert!(at(0.0).is_none(), "a newborn was scored");
    // The published default for a patient whose age nobody wrote down.
    assert!(news::applies_to_age(None), "an undeclared age is scored as an adult");
}

/// Nothing here invents a paediatric score. PEWS is a different instrument with age-banded
/// charts and local variation, and picking one is a clinical decision, not a rendering one — so
/// the honest output is no number and a sentence saying which instrument is missing.
#[test]
fn what_stands_in_for_the_score_does_not_reassure() {
    let m = news::NOT_VALIDATED.to_lowercase();
    assert!(m.contains("not validated"), "{m:?}");
    assert!(m.contains("16"), "{m:?} does not say who it is not validated for");
    for soothing in ["stable", "normal", "no concern", "low risk", "fine", "well"] {
        assert!(!m.contains(soothing), "{m:?} reads as reassurance about a child who may be sick");
    }
}

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
    let s = news::score(&news::Obs { age_years: ADULT, rr: 16.0, spo2: 98.0, on_oxygen: false, sbp: 120.0, hr: 70.0, temp: 36.8, gcs: 15 }).expect("an adult is scored");
    assert_eq!(s.total, 0);
    assert_eq!(s.band, news::Band::Low);
}

/// EP1's patient shortly after the sting: tachycardic, hypotensive, hypoxic, on oxygen.
#[test]
fn a_patient_in_anaphylaxis_scores_high_and_says_so() {
    let s = news::score(&news::Obs { age_years: ADULT, rr: 28.0, spo2: 90.0, on_oxygen: true, sbp: 86.0, hr: 128.0, temp: 36.9, gcs: 15 }).expect("an adult is scored");
    // 3 (rr) + 3 (spo2) + 2 (oxygen) + 3 (sbp) + 2 (hr) + 0 + 0
    assert_eq!(s.total, 13);
    assert_eq!(s.band, news::Band::High);
}

/// A single 3 in any one observation escalates on its own, even when the total looks calm. That
/// rule exists because one catastrophic derangement is not averaged away by six normal ones, and
/// averaging is exactly what the old percentage did.
#[test]
fn one_extreme_observation_escalates_on_its_own() {
    let s = news::score(&news::Obs { age_years: ADULT, rr: 16.0, spo2: 98.0, on_oxygen: false, sbp: 88.0, hr: 70.0, temp: 36.8, gcs: 15 }).expect("an adult is scored");
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
    let s = news::score(&news::Obs { age_years: ADULT, rr: 0.0, spo2: 0.0, on_oxygen: true, sbp: 0.0, hr: 0.0, temp: -50.0, gcs: 3 }).expect("an adult is scored");
    assert_eq!(s.total, 3 + 3 + 2 + 3 + 3 + 3 + 3);
    let s = news::score(&news::Obs { age_years: ADULT, rr: 999.0, spo2: 200.0, on_oxygen: false, sbp: 999.0, hr: 999.0, temp: 99.0, gcs: 15 }).expect("an adult is scored");
    assert_eq!(s.total, (3 + 3 + 3 + 2));
}


/// **Whose age the score is built from, when the ward put somebody else in the bed.**
///
/// `applies_to_age` treats an unknown age as an adult, and says in its own doc comment why that is
/// safe: *the case table declares an age for every case and a test fails if one is missing;
/// without it, "no age" would become the way a child gets scored as an adult again.*
///
/// Vitals World broke that premise without touching this file. Its patients are not cases from
/// that table — the factory draws a person and the ward places her on a case — so the lookup by
/// case id returns nothing, "nothing" is read as an adult, and on 22 ก.ย. Sagal Abdi, eight years
/// old, was in bed 3 on production carrying a NEWS2 of an adult table. Fadumo Jama, two, is in the
/// tutorial screenshots reading 15 · HIGH RISK.
///
/// It is the third time this family has bitten: the card that called a child by the case's age in
/// ก.ย. 16, the receipt title last night, and now a clinical score. Each of the first two was
/// closed where it was noticed. So the rule is named here rather than patched there: **the patient
/// in the bed decides, and the patient the case was authored about never overrides her.**
///
/// Under sixteen the answer is that there is no score — not a paediatric one. Which PEWS to use is
/// a clinical decision, it is question 3.2 on the form we are asking an advisor right now, and a
/// ward that answers it for him while asking him would be inventing a number nobody checked.
#[test]
fn the_patient_in_the_bed_decides_the_age_the_score_is_built_from() {
    use vitals_web::news2::{age_for, applies_to_age, score, Obs, NOT_VALIDATED};

    // A case authored about a woman of forty, with a girl of eight placed on it.
    assert_eq!(age_for(Some(8), Some(40.0)), Some(8.0),
               "the age is hers, not the age of the patient the case was written about");
    assert_eq!(age_for(Some(71), Some(40.0)), Some(71.0), "and in the other direction too");

    // No placed patient: the season's own case, answered from the case table as it always was.
    assert_eq!(age_for(None, Some(40.0)), Some(40.0));
    assert_eq!(age_for(None, None), None, "an authored case with no age is unchanged — adult");

    // The ward's failure, stated as the rule that ends it: a placed patient is never unknown.
    assert!(age_for(Some(8), None).is_some(),
            "a ward patient's age can never come back unknown — unknown is how she was scored as \
             an adult");

    // And what the screen gets for her. `score` answers `None` for a patient this instrument does
    // not read — which is the whole point: not zero, not unknown, not reassuring.
    let obs = |age| Obs { age_years: age, rr: 28.0, spo2: 96.0, on_oxygen: false,
                          sbp: 98.0, hr: 118.0, temp: 37.4, gcs: 15 };

    let hers = obs(age_for(Some(8), Some(40.0)));
    assert!(!applies_to_age(hers.age_years), "eight is not an adult");
    assert!(score(&hers).is_none(), "a girl of eight is not given an adult score");

    // The same observations read through the case's patient — which is what production did — come
    // back scored, and high. This is the bug in one assertion: identical vitals, two answers,
    // decided entirely by which patient the age was taken from.
    let the_cases = obs(Some(40.0));
    assert!(score(&the_cases).is_some(),
            "the same observations on a woman of forty are scored, and that is correct");

    // The sentence that goes where the number would have been. A blank reads as nothing to worry
    // about on a child who may be very sick.
    assert!(NOT_VALIDATED.contains("under 16"), "{NOT_VALIDATED}");
}
