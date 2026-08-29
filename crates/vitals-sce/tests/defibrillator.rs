//! The defibrillator, as physiology rather than as content.
//!
//! `SceState::defibrillate` has existed since the rhythm did, and until this file was written it
//! was called by exactly one test. In the running product the kit button minted the words
//! `"defibrillate 200 j"` and posted them to each case's own intervention matcher — so what a
//! shock *did* was whatever the case author happened to have written, and three of the four
//! season episodes had written nothing at all. Pressing the defibrillator on a child in cardiac
//! arrest charted nothing, scored nothing and appeared in no debrief.
//!
//! What this file pins is the other arrangement: one answer, from the engine, for every case on
//! the shelf.
//!
//!   * the rhythm decides, and the rhythm is a fact the engine holds;
//!   * every shock leaves the same shape behind — one [`vitals_sce::runtime::SHOCK`] row and one
//!     beat — so that silence can never be the tell that a shock was the wrong one;
//!   * the sentence that says what it *cost* is a harm, which the exam seals until the bell;
//!   * and the automaton moves afterwards, so a case can key an edge on the rhythm a shock
//!     produced.

use vitals_sce::runtime::{Rhythm, ShockResult, SHOCK};
use vitals_sce::{NarrativeBeat, PatientStatus, Sce, SceState};

/// The smallest case that can be in any rhythm on demand: one state per rhythm, and a `to_state`
/// on each so a test can put the patient where it needs her without waiting.
fn case_in(rhythm: &str) -> SceState {
    let json = format!(
        r#"{{
          "vitals0": {{"hr":80,"sbp":120,"dbp":78,"spo2":97,"rr":16,"temp":37.0,"gcs":15}},
          "initial_state": "here",
          "states": [{{"id":"here","status":"critical","rhythm":"{rhythm}"}}],
          "outcomes": []
        }}"#
    );
    SceState::new(Sce::from_json(&json).expect("fixture"))
}

fn rows(st: &SceState, kind: &str) -> Vec<String> {
    st.events().iter().filter(|e| e.kind == kind).map(|e| e.text.clone()).collect()
}

fn thresholds(beats: &[NarrativeBeat]) -> Vec<String> {
    beats
        .iter()
        .filter_map(|b| match b {
            NarrativeBeat::Threshold(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// VF and pulseless VT convert. Nothing else does, and nothing else ever did — this is
/// `Rhythm::shockable`, asserted through the door a learner actually presses.
#[test]
fn only_a_shockable_rhythm_converts() {
    for (rhythm, want) in [
        ("vf", ShockResult::Converted),
        ("vt", ShockResult::Converted),
        ("pea", ShockResult::NotShockable),
        ("asystole", ShockResult::NotShockable),
        ("sinus", ShockResult::Perfusing),
    ] {
        let mut st = case_in(rhythm);
        // Asking must not change the answer, and must not cost the patient a shock.
        assert_eq!(st.shock_result(), want, "{rhythm}: shock_result");
        let before = st.vitals.rhythm;
        assert_eq!(st.shock_result(), want, "{rhythm}: asking twice gave two answers");
        assert_eq!(st.vitals.rhythm, before, "{rhythm}: asking delivered a shock");

        let (got, _) = st.defibrillate(200.0);
        assert_eq!(got, want, "{rhythm}: defibrillate");
        let after = st.vitals.rhythm;
        if want == ShockResult::Converted {
            assert_eq!(after, Rhythm::Sinus, "{rhythm} was shockable and did not convert");
        } else {
            assert_eq!(after, Rhythm::parse(rhythm).unwrap(), "{rhythm} moved when it should not");
        }
    }
}

/// **The silence rule, in the one place the engine writes its own beats.**
///
/// `conformance/README.md`: a reply that is one line shorter after the wrong answer is the answer
/// key. Every shock emits exactly one threshold beat, before anything is known about what it did,
/// and every shock leaves exactly one row on the chart. What separates a right shock from a wrong
/// one is a *harm*, and a harm is sealed.
#[test]
fn every_shock_says_the_same_amount_regardless_of_whether_it_was_right() {
    let mut shapes = Vec::new();
    for rhythm in ["vf", "vt", "pea", "asystole", "sinus"] {
        let mut st = case_in(rhythm);
        let (_, beats) = st.defibrillate(200.0);
        assert_eq!(
            thresholds(&beats),
            vec![SceState::SHOCK_BEAT.to_string()],
            "{rhythm}: a shock announced itself differently"
        );
        assert_eq!(rows(&st, SHOCK).len(), 1, "{rhythm}: not exactly one chart row");
        shapes.push((rhythm, rows(&st, SHOCK).remove(0)));
    }
    // The rows differ only in the two rhythms they name — both of which the strip is already
    // drawing. Same template, same shape, same length in words; none of them says anybody was
    // wrong, and none of them is shorter for the shock that could not work.
    let mut lengths = std::collections::BTreeSet::new();
    for (rhythm, line) in &shapes {
        assert_eq!(
            line,
            &format!(
                "defibrillate 200 J into {rhythm} — rhythm now {}",
                if ["vf", "vt"].contains(rhythm) { "sinus" } else { rhythm }
            ),
            "{rhythm}: the row is not the one template"
        );
        for word in ["harm", "wrong", "mistake", "error", "should", "not shockable", "danger"] {
            assert!(
                !line.to_lowercase().contains(word),
                "{rhythm}: the chart row grades the order — {line:?}"
            );
        }
        lengths.insert(line.split_whitespace().count());
    }
    assert_eq!(lengths.len(), 1, "the rows are not all the same length: {shapes:?}");
}

/// What a wrong shock cost, on the harm list, where the exam can seal it.
#[test]
fn a_shock_that_could_not_work_records_what_it_cost() {
    for (rhythm, cost) in [
        ("pea", "compressions and adrenaline"),
        ("asystole", "compressions and adrenaline"),
        ("sinus", "ventricular fibrillation"),
    ] {
        let mut st = case_in(rhythm);
        let (_, beats) = st.defibrillate(200.0);
        let harms = rows(&st, "harm");
        assert_eq!(harms.len(), 1, "{rhythm}: {harms:?}");
        assert_eq!(st.harm_events, harms, "{rhythm}: the harm list and the chart disagree");
        assert!(harms[0].contains(cost), "{rhythm}: the sentence does not say what it cost: {harms:?}");
        // ep2's rubric earns five marks for `no_harm` on this substring and it is the only
        // needle any sheet in the repo has for a wrong shock. The wording moved out of the
        // scenario file and into the engine; it may not take the marks with it.
        assert!(
            harms[0].contains("unsynchronised shock"),
            "the `no_harm` needle ep2 scores on is gone: {harms:?}"
        );
        assert!(
            beats.iter().any(|b| matches!(b, NarrativeBeat::Harm(_))),
            "{rhythm}: the harm never reached the feed, so nothing can seal it"
        );
    }
    // …and a shock that worked is not a harm.
    let mut st = case_in("vf");
    st.defibrillate(200.0);
    assert!(st.harm_events.is_empty(), "converting VF was charted as harm: {:?}", st.harm_events);
}

/// The energy is the learner's, not the engine's.
#[test]
fn the_chart_quotes_the_joules_that_were_dialled() {
    let mut st = case_in("vf");
    st.defibrillate(360.0);
    assert_eq!(rows(&st, SHOCK), vec!["defibrillate 360 J into vf — rhythm now sinus"]);
}

/// **The reason `Cond::Rhythm` exists.**
///
/// The engine gives a rhythm back and cannot know what the case wants next. Without an edge that
/// reads the rhythm, a converted patient stays in the state named for the rhythm she is no longer
/// in — which is how a defibrillated EP2 used to reach asystole ninety seconds later with sinus
/// on the monitor.
#[test]
fn a_case_can_key_an_edge_on_the_rhythm_a_shock_produced() {
    let json = r#"{
      "vitals0": {"hr":0,"sbp":0,"dbp":0,"spo2":0,"rr":0,"temp":36.0,"gcs":3},
      "initial_state": "vf",
      "states": [
        {"id":"vf","status":"arrest","rhythm":"vf",
         "transitions":[{"to_state":"after","when":{"rhythm":"sinus"},
                         "do":[{"set":{"sbp":88,"hr":104}},{"beat":"rosc"}]}]},
        {"id":"after","status":"improving","rhythm":"sinus"}
      ],
      "outcomes": []
    }"#;
    let mut st = SceState::new(Sce::from_json(json).expect("fixture"));
    st.tick(1.0);
    assert_eq!(st.status, PatientStatus::Arrest, "the fixture did not start in arrest");

    let (r, beats) = st.defibrillate(200.0);
    assert_eq!(r, ShockResult::Converted);
    // The edge was taken inside the same call. A shock that converts a rhythm and leaves the
    // automaton where it was is the defect this replaced.
    assert_eq!(st.vitals.rhythm, Rhythm::Sinus);
    assert_eq!(st.status, PatientStatus::Improving, "the case never left the state it was in");
    assert_eq!(st.vitals.sbp, 88.0, "the case's own idea of what ROSC looks like never ran");
    assert!(thresholds(&beats).contains(&"rosc".to_string()), "{beats:?}");
}

/// A rhythm condition reads the *live* rhythm, both ways round.
#[test]
fn a_rhythm_condition_is_true_only_while_the_patient_is_in_it() {
    let json = r#"{
      "vitals0": {"hr":80,"sbp":120,"dbp":78,"spo2":97,"rr":16,"temp":37.0,"gcs":15},
      "initial_state": "a",
      "states": [
        {"id":"a","status":"critical","rhythm":"vf",
         "dynamics":[{"var":"spo2","rate_per_min":-60,"when":{"not":{"rhythm":"sinus"}}}]},
        {"id":"b","status":"stable"}
      ],
      "outcomes": []
    }"#;
    let mut st = SceState::new(Sce::from_json(json).expect("fixture"));
    st.tick(60.0);
    assert!(st.vitals.spo2 < 40.0, "the guard never let the dynamic run: {}", st.vitals.spo2);
    st.defibrillate(200.0);
    let held = st.vitals.spo2;
    st.tick(60.0);
    assert_eq!(st.vitals.spo2, held, "the dynamic kept running after the rhythm changed");
}

/// An unparseable rhythm is refused at the door, because it would otherwise be silent: a
/// condition that can never hold is an edge that never fires, and the case runs on looking fine.
#[test]
fn a_rhythm_nobody_can_parse_is_an_authoring_error() {
    let json = r#"{
      "vitals0": {"hr":80,"sbp":120,"dbp":78,"spo2":97,"rr":16,"temp":37.0,"gcs":15},
      "initial_state": "a",
      "states": [{"id":"a","status":"stable",
                  "transitions":[{"outcome":"win_discharge","when":{"rhythm":"ventricular fib"}}]}],
      "outcomes": [{"id":"win_discharge","kind":"win"}]
    }"#;
    let errs = Sce::from_json(json).expect("parses").validate();
    assert!(
        errs.iter().any(|e| e.contains("unknown rhythm 'ventricular fib'")),
        "a rhythm the engine cannot read was accepted: {errs:?}"
    );
}

/// Nothing more happens to a patient the case has finished with — the guard `fire` and `tick`
/// both open with, and the one that stops a post-bell shock landing on the record.
#[test]
fn a_shock_after_the_ending_changes_nothing() {
    let json = r#"{
      "vitals0": {"hr":80,"sbp":120,"dbp":78,"spo2":97,"rr":16,"temp":37.0,"gcs":15},
      "initial_state": "a",
      "states": [{"id":"a","status":"critical","rhythm":"vf",
                  "transitions":[{"outcome":"death_arrest","when":{"var":"t_elapsed","op":"ge","value":1}}]}],
      "outcomes": [{"id":"death_arrest","kind":"death"}]
    }"#;
    let mut st = SceState::new(Sce::from_json(json).expect("fixture"));
    st.tick(2.0);
    assert!(st.outcome().is_some(), "the fixture did not end");
    let before = st.events().len();
    let (_, beats) = st.defibrillate(200.0);
    assert!(beats.is_empty(), "a shock after the ending emitted beats: {beats:?}");
    assert_eq!(st.events().len(), before, "a shock after the ending reached the chart");
    assert!(st.harm_events.is_empty());
}
