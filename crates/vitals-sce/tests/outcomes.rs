//! Every ending a shipped case can reach is an ending the engine actually plays.
//!
//! The failure this pins is not loud. `OutcomeDef.kind` is a free `String`, so a case that says
//! `"kind": "lose"` parses, validates, and plays — and then `terminate` matches `"win"`, matches
//! `"death"`, and falls off the end of the match having done nothing. No `Dead` status, no zeroed
//! vitals, no asystole. The run reaches its terminal beat with the monitor still sweeping a pulse
//! of 128 across a patient the script has just killed, and the only person who finds out is the
//! examiner watching the screen.
//!
//! Four of the five scenario files said `"lose"`. They said it from the day they were written.
//!
//! The same hole has a quieter half: `outcome_enum` names four ids and falls back for anything
//! else, so EP4's `death_at_home` played as `death_arrest` — the "dies at home" line existed in
//! the page and could not be reached, and the discharged-as-anxiety ending was indistinguishable
//! from arresting in the department.
//!
//! So both lists live in the engine now, and `validate()` checks against them. These tests hold
//! the check honest in both directions: it must reject the two shapes that got through, and it
//! must pass every case that ships.

use std::path::PathBuf;
use vitals_sce::schema::OUTCOME_KINDS;
use vitals_sce::{PatientStatus, Sce, SceState};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `.sce.json` the repository ships: five scenarios, twelve stations, the conformance EP1.
fn every_shipped_case() -> Vec<(String, Sce)> {
    let r = root();
    let mut files: Vec<PathBuf> = vec![r.join("conformance/sce-anaphylaxis-ep1.json")];
    for dir in ["demo/scenarios", "demo/stations"] {
        let mut found: Vec<PathBuf> = std::fs::read_dir(r.join(dir))
            .unwrap_or_else(|e| panic!("{dir}: {e}"))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        found.sort();
        files.extend(found);
    }
    assert!(files.len() >= 17, "the season lost files: only {} found", files.len());
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let json = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            (name.clone(), Sce::from_json(&json).unwrap_or_else(|e| panic!("{name}: {e}")))
        })
        .collect()
}

/// The smallest case that can end: one state, one outcome, whatever `id`/`kind` the caller wants
/// to try to smuggle past the validator.
fn one_ending(id: &str, kind: &str) -> Sce {
    Sce::from_json(&format!(
        r#"{{
             "vitals0": {{ "hr": 80, "sbp": 120, "dbp": 80, "spo2": 98, "rr": 14, "temp": 37.0, "gcs": 15 }},
             "initial_state": "s",
             "states": [{{ "id": "s", "status": "stable",
                           "transitions": [{{ "outcome": "{id}",
                                              "when": {{ "var": "t_in_state", "op": "ge", "value": 1 }} }}] }}],
             "outcomes": [{{ "id": "{id}", "kind": "{kind}" }}]
           }}"#
    ))
    .expect("the fixture is well-formed JSON whatever it says about outcomes")
}

/// The bug, as a unit. `"lose"` is a word a human writes and the engine does not read.
#[test]
fn a_kind_the_engine_cannot_read_is_refused() {
    let sce = one_ending("death_arrest", "lose");
    let errs = sce.validate();
    assert!(
        errs.iter().any(|e| e.contains("kind 'lose'")),
        "an outcome kind the engine ignores validated clean: {errs:?}"
    );

    // …and the three it does read still pass, so this is a guardrail and not a wall.
    for kind in OUTCOME_KINDS {
        let sce = one_ending("death_arrest", kind);
        assert!(sce.validate().is_empty(), "'{kind}' is a kind the engine acts on: {:?}", sce.validate());
    }
}

/// The quieter half: an id nobody maps plays as some other ending, cutscene and all.
#[test]
fn an_outcome_id_the_engine_cannot_name_is_refused() {
    let sce = one_ending("death_at_home", "death");
    let errs = sce.validate();
    assert!(
        errs.iter().any(|e| e.contains("death_at_home")),
        "an unmapped outcome id validated clean: {errs:?}"
    );
}

/// Nothing shipped may carry either shape. This is the test that would have caught all five files.
#[test]
fn every_shipped_case_ends_in_a_way_the_engine_plays() {
    for (name, sce) in every_shipped_case() {
        assert!(!sce.outcomes.is_empty(), "{name}: a case with no ending at all");
        assert!(sce.validate().is_empty(), "{name}: {:?}", sce.validate());
    }
}

/// The consequence, played rather than asserted about: drive every case that can die to its
/// death outcome and check the body behaves like one. This is the screen the examiner sees.
#[test]
fn a_case_that_reaches_a_death_outcome_flatlines() {
    let mut died: Vec<String> = Vec::new();
    for (name, sce) in every_shipped_case() {
        let deaths: Vec<String> = sce
            .outcomes
            .iter()
            .filter(|o| o.kind == "death")
            .map(|o| o.id.clone())
            .collect();
        if deaths.is_empty() {
            continue;
        }
        // The neglected run: never touch the patient, let the dynamics take their course. Every
        // case in the season kills on neglect except the two that open `stable` and wait for an
        // order — those are skipped rather than forced, because forcing them would be testing a
        // fixture instead of the case.
        let mut st = SceState::new(sce);
        for _ in 0..400 {
            st.tick(5.0);
            if st.outcome().is_some() {
                break;
            }
        }
        let Some(id) = st.outcome_id().map(str::to_string) else { continue };
        if !deaths.contains(&id) {
            continue;
        }
        assert_eq!(st.status, PatientStatus::Dead, "{name}: reached '{id}' and is not dead");
        let v = st.vitals;
        assert_eq!(v.hr, 0.0, "{name}: '{id}' left a heart rate on the monitor");
        assert_eq!(v.rr, 0.0, "{name}: '{id}' left the chest rising");
        assert_eq!(v.sbp, 0.0, "{name}: '{id}' left a blood pressure");
        assert_eq!(v.spo2, 0.0, "{name}: '{id}' left a saturation to read");
        assert_eq!(v.rhythm.as_str(), "asystole", "{name}: '{id}' left a rhythm on the strip");
        died.push(name);
    }

    // And the test is not quietly passing by skipping everything. The four scenarios named here
    // are the four that shipped `"lose"`: if any of them stops reaching a death outcome on
    // neglect, this test has lost the thing it was written to watch.
    for must in [
        "ep2-stemi.json",
        "ep3-epiglottitis.json",
        "ep4-pulmonary-embolism.json",
        "ep5-the-night-the-stars-fell.json",
        "sce-anaphylaxis-ep1.json",
    ] {
        assert!(died.iter().any(|d| d == must), "{must} never reached a death outcome — checked: {died:?}");
    }
}
