//! What a monitor can actually *read* off a patient, as opposed to what the model knows.
//!
//! The automaton holds a full physiological vector at every instant, including during an arrest.
//! A bedside monitor does not: two of those numbers are measurements of flowing blood, and when
//! there is no flow there is no measurement — the pulse oximeter has nothing pulsatile to detect
//! and the cuff has nothing to occlude. The engine already says this out loud, once, in the
//! branch that kills a patient:
//!
//! > *No pulse for an oximeter to read. A saturation is a measurement of flowing blood.*
//!
//! That rule was attached to **death** when it belongs to **pulselessness**. In PEA, VF and
//! pulseless VT the patient is not dead and there is still no cardiac output, and the bay's rail
//! went on printing `SpO₂ 80%` and `BP 54/54` through the whole arrest while the bedside device —
//! which has always keyed on the rhythm — printed `--`. Two panels on one screen disagreeing
//! about whether a patient has a pulse is worse than either answer alone.
//!
//! So the rule lives here, once, and both panels are served from it. The server never ships a
//! flow-derived number for a patient who has no flow, which means no renderer can invent one and
//! `curl` can check the claim without opening a browser.
//!
//! **HR keeps its number.** Pulseless electrical activity is organised complexes marching along
//! at a countable rate with no output at all — that disagreement *is* the finding, and hiding the
//! rate would hide the trap the station is teaching.
//!
//! **Known limitation.** `rr` reports 0 in an arrest because a patient in cardiac arrest is not
//! breathing; agonal gasps are not a respiratory rate. A patient being ventilated through an
//! arrest would show the bag's rate on a real impedance trace, and the engine has no ventilator
//! rate to report — so this reads as apnoea throughout. That is the conservative error: a flat
//! trace understates, a calm 28-per-minute over a cardiac arrest is a lie.

use vitals_sce::Vitals;

/// One patient's numbers as a monitor would print them. `None` means "no reading", not "zero".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// Electrical, so it survives an arrest. See the module note.
    pub hr: f64,
    /// Needs a pulsatile signal at the probe.
    pub spo2: Option<f64>,
    /// Needs a pulse under the cuff.
    pub sbp: Option<f64>,
    pub dbp: Option<f64>,
    /// 0 in an arrest — apnoea, and the flat impedance trace that goes with it.
    pub rr: f64,
    pub temp: f64,
    pub gcs: u8,
    /// Whether the heart is producing output. The one fact the two panels must agree on.
    pub pulse: bool,
    pub rhythm: &'static str,
    pub shockable: bool,
}

impl Reading {
    pub fn of(v: &Vitals) -> Reading {
        let pulse = v.rhythm.perfusing();
        // `then_some` and not a match, so adding a third flow-derived number is one line and
        // cannot accidentally be added to the wrong side of the rule.
        let flowing = |x: f64| pulse.then_some(x);
        Reading {
            hr: v.hr,
            spo2: flowing(v.spo2),
            sbp: flowing(v.sbp),
            dbp: flowing(v.dbp),
            rr: if pulse { v.rr } else { 0.0 },
            temp: v.temp,
            gcs: v.gcs,
            pulse,
            rhythm: v.rhythm.as_str(),
            shockable: v.rhythm.shockable(),
        }
    }

    /// The same reading at the precision a screen prints it. Kept apart from [`Reading::of`] so
    /// nothing downstream has to guess whether it is holding a rounded number or a live one.
    pub fn rounded(self) -> Reading {
        Reading {
            hr: self.hr.round(),
            spo2: self.spo2.map(f64::round),
            sbp: self.sbp.map(f64::round),
            dbp: self.dbp.map(f64::round),
            rr: self.rr.round(),
            temp: (self.temp * 10.0).round() / 10.0,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vitals_sce::runtime::Rhythm;

    fn arresting(rhythm: Rhythm) -> Vitals {
        // EP1's arrest state, as the automaton actually leaves it: the numbers are frozen at
        // whatever they held the instant the rhythm changed, which is why the rail was printing
        // a saturation of 80 and a blood pressure of 54/54 over a patient with no cardiac output.
        Vitals { hr: 128.0, sbp: 54.0, dbp: 54.0, spo2: 80.0, rr: 28.0, temp: 37.0, gcs: 3, rhythm }
    }

    #[test]
    fn a_perfusing_patient_reads_every_number() {
        let v = arresting(Rhythm::Sinus);
        let r = Reading::of(&v);
        assert!(r.pulse);
        assert_eq!(r.spo2, Some(80.0));
        assert_eq!((r.sbp, r.dbp), (Some(54.0), Some(54.0)));
        assert_eq!(r.rr, 28.0);
    }

    /// The whole point. Three rhythms, one rule, and PEA is the one that matters most: the ECG
    /// looks survivable and there is no output behind it.
    #[test]
    fn no_pulse_means_no_saturation_and_no_blood_pressure() {
        for rhythm in [Rhythm::Pea, Rhythm::Vf, Rhythm::Vt, Rhythm::Asystole] {
            let r = Reading::of(&arresting(rhythm));
            assert!(!r.pulse, "{rhythm:?} reads as perfusing");
            assert_eq!(r.spo2, None, "{rhythm:?} still reports a saturation");
            assert_eq!(r.sbp, None, "{rhythm:?} still reports a systolic");
            assert_eq!(r.dbp, None, "{rhythm:?} still reports a diastolic");
            assert_eq!(r.rr, 0.0, "{rhythm:?} is still breathing calmly through a cardiac arrest");
        }
    }

    /// PEA's rate is the finding, not a leak. Blanking it would hide the trap.
    #[test]
    fn the_heart_rate_survives_the_arrest_because_it_is_electrical() {
        let r = Reading::of(&arresting(Rhythm::Pea));
        assert_eq!(r.hr, 128.0);
        assert_eq!(r.rhythm, "pea");
        assert!(!r.shockable, "PEA must never be offered to a defibrillator");
    }

    #[test]
    fn only_the_two_shockable_rhythms_say_so() {
        for (rhythm, want) in [
            (Rhythm::Vf, true),
            (Rhythm::Vt, true),
            (Rhythm::Pea, false),
            (Rhythm::Asystole, false),
            (Rhythm::Sinus, false),
        ] {
            assert_eq!(Reading::of(&arresting(rhythm)).shockable, want, "{rhythm:?}");
        }
    }

    #[test]
    fn rounding_does_not_resurrect_a_missing_reading() {
        let r = Reading::of(&arresting(Rhythm::Pea)).rounded();
        assert_eq!(r.spo2, None);
        assert_eq!(r.sbp, None);
        // …and a present one is rounded rather than dropped.
        let ok = Reading::of(&Vitals { spo2: 93.6, ..arresting(Rhythm::Sinus) }).rounded();
        assert_eq!(ok.spo2, Some(94.0));
    }
}
