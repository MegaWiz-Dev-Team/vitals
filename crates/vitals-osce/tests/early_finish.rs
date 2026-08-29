//! **Finishing early must never be worth more than playing on.**
//!
//! The failure this file exists to catch is not hypothetical, and `osce-d4` is the worked
//! example. A candidate who takes the history, the lactate and the cultures, opens two lines,
//! runs fluids, gives the antibiotic, catheterises, plans the source and books the bed — and
//! never starts the vasopressor — scores **29 of 40**, which is 7250 basis points against a
//! 7000 pass bar. She arrests at sixteen simulated minutes, `death_cap` takes the total to 27,
//! and the run fails. A finish button that froze the clock at fourteen minutes would have paid
//! that candidate a star over a body.
//!
//! It cannot, because ending does not freeze anything: it runs the encounter on. The two
//! properties below are the whole of the guarantee.
//!
//!   * **No advantage.** Ending at any moment after the last order scores exactly what ending
//!     at any other moment scores — including never ending at all and letting the case run out.
//!     There is no moment worth choosing, so there is nothing to game.
//!   * **No penalty.** Which is the same sentence read the other way, and why an early finish
//!     needs no forgiveness written into the mark sheet: the sheet is already the sheet the
//!     candidate would have got.

use vitals_osce::sheet_for_run;
use vitals_replay::{resume, rung, Step};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn sce(id: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}/demo/stations/{id}.sce.json")).expect("scenario")
}
fn rubric(id: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}/demo/rubrics/{id}.json")).expect("rubric")
}

/// Orders at ten-second intervals, the way a candidate gives them.
fn orders(ids: &[&str]) -> Vec<Step> {
    let mut tape = Vec::new();
    for o in ids {
        tape.push(Step::Act { text: (*o).into(), id: (*o).into() });
        for _ in 0..5 {
            tape.push(Step::Tick(2.0));
        }
    }
    tape
}

/// Stand at the bedside until `t`, stopping the moment the run is over — which is what the
/// server does, because a finished run takes nothing more onto its tape.
fn wait_until(sce: &str, tape: &mut Vec<Step>, t: f64, limit: f64) {
    let mut st = vitals_sce::SceState::new(vitals_sce::Sce::from_json(sce).expect("sce"));
    let mut clock = 0.0;
    for s in tape.iter() {
        if let Step::Tick(dt) = s {
            st.tick(*dt);
            clock += dt;
        } else if let Step::Act { id, .. } = s {
            st.apply_id(id);
        }
    }
    while clock < t && st.outcome().is_none() && clock < limit {
        st.tick(2.0);
        tape.push(Step::Tick(2.0));
        clock += 2.0;
    }
}

/// `(earned, max, bps, capped_from, outcome)` — everything the sheet says about a run.
fn marked(id: &str, tape: &[Step]) -> (u16, u16, u32, Option<u16>, Option<String>) {
    let j = sce(id);
    let (rub, det) = sheet_for_run(&j, tape, &rubric(id)).expect("sheet");
    let (st, _) = resume(&j, tape).expect("replay");
    assert_eq!(rub.pass_bps, 7_000, "{id}: the pass bar moved and this test reads it");
    (det.earned, det.max, det.bps(), det.capped_from, st.outcome_id().map(str::to_string))
}

/// The exact run described at the top of this file, ended at every moment a candidate could
/// press the button. The arrest is on the tape whichever moment is chosen.
#[test]
fn the_run_that_would_arrest_still_arrests_however_early_it_is_ended() {
    let id = "osce-d4";
    let j = sce(id);
    let limit = 14.0 * 60.0; // what the card advertises
    let given = orders(&["ask_niece", "exam_perfusion", "exam_flank", "lactate", "cultures",
                         "urinalysis", "two_lines", "fluids", "antibiotics", "catheter",
                         "source_control", "icu_bed", "dx_sepsis"]);

    // Played all the way out with nobody pressing anything: she arrests, and the cap bites.
    let (played, _) = rung(&j, &given, limit).expect("ring");
    let full = marked(id, &played);
    assert_eq!(full.4.as_deref(), Some("death_arrest"), "the worked example stopped working");
    assert_eq!(full.3, Some(29), "the pre-cap total moved — this test is quoting it");
    assert!(full.2 < 7_000, "the cap did not bite: {}bps", full.2);
    // The cheat this design exists to refuse: 29/40 is a pass, and 29 is what freezing the
    // clock would have banked.
    assert!(29 * 10_000 / full.1 as u32 >= 7_000, "the frozen score would not have been a pass");

    for press_min in [2.5, 4.0, 6.0, 9.0, 13.0, 13.9] {
        let mut tape = given.clone();
        wait_until(&j, &mut tape, press_min * 60.0, limit);
        let (ended, _) = rung(&j, &tape, limit).expect("ring");
        assert_eq!(ended, played, "{id}: ending at {press_min} min is a different run");
        assert_eq!(marked(id, &ended), full, "{id}: ending at {press_min} min marks differently");
    }
}

/// The same statement, over every station and every kind of run: **there is no moment worth
/// pressing it at.** A sheet that ever came out higher for an earlier press would fail here,
/// and so would one that came out lower — a penalty for finishing early is as dishonest as a
/// reward for it.
#[test]
fn no_moment_of_finishing_is_worth_more_or_less_than_any_other() {
    // Three shapes of run per station: nothing done, the definitive order given, and a
    // half-worked-up patient. Ids that a case does not define are inert, which is what makes
    // one list legal across twelve scenarios.
    let shapes: &[&[&str]] = &[
        &[],
        &["adrenaline_im", "adrenaline_child", "nsaid", "dexamethasone", "neb_salbutamol",
          "antibiotics", "heparin", "endoscopy", "cath_lab", "norepinephrine"],
        &["ask_pain", "ask_allergy", "ask_onset", "exam_chest", "exam_skin", "ecg", "cxr",
          "oxygen", "cbc"],
    ];
    for id in ["osce-a", "osce-a2", "osce-b", "osce-b2", "osce-b3", "osce-c", "osce-c2",
               "osce-c3", "osce-d", "osce-d2", "osce-d3", "osce-d4"] {
        let j = sce(id);
        for limit_min in [8.0, 10.0, 12.0, 14.0] {
            let limit = limit_min * 60.0;
            for shape in shapes {
                let given = orders(shape);
                let (played, _) = rung(&j, &given, limit).expect("ring");
                let want = marked(id, &played);
                for press_min in [0.0, 1.0, 3.0, 5.0, 8.0, 12.0, 17.0] {
                    let mut tape = given.clone();
                    wait_until(&j, &mut tape, press_min * 60.0, limit);
                    let (ended, _) = rung(&j, &tape, limit).expect("ring");
                    let got = marked(id, &ended);
                    assert_eq!(
                        got, want,
                        "{id} @ {limit_min} min: finishing at {press_min} min scores \
                         {}/{} ({}bps) where playing on scores {}/{} ({}bps) — an early \
                         finish must be worth neither more nor less",
                        got.0, got.1, got.2, want.0, want.1, want.2
                    );
                }
            }
        }
    }
}
