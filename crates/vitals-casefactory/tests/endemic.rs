//! The six endemic World cases are the first targets: every one compiles, honestly, under the
//! archetype its physiology fits, dies untreated, and wins along its own plan.

mod common;

use vitals_casefactory::{compile, Source};

#[test]
fn all_six_endemic_cases_compile_and_are_marked_endemic() {
    let Some(dir) = common::embla_dir() else { return common::skip("embla-cases not present"); };
    let mut failures = Vec::new();
    for id in common::ENDEMIC_SIX {
        let Some(json) = common::show(&dir, common::ENDEMIC_REF, id) else {
            return common::skip("endemic branch not present");
        };
        match compile(&json, Source::of("embla-cases", common::ENDEMIC_REF, &json)) {
            Ok(pack) => {
                assert!(pack.endemic, "{id} is tagged endemic");
                assert!(pack.provisional);
                assert!(pack.country.is_some(), "{id} carries a country");
                assert!(pack.replay.untreated_death_sec > 0.0);
                assert!(pack.replay.win_sec > 0.0);
                assert!(!pack.voice.is_empty(), "{id} has a voice");
                eprintln!(
                    "{id}: {} — dies untreated at {} s, wins ({}) at {} s via {} orders, golden {}/{}",
                    pack.archetype, pack.replay.untreated_death_sec, pack.replay.win_outcome, pack.replay.win_sec,
                    pack.replay.win_path.len(), pack.replay.golden_score.earned, pack.replay.golden_score.max
                );
            }
            Err(e) => failures.push(format!("{}: {}", e.case_id, e.reason)),
        }
    }
    assert!(failures.is_empty(), "refused:\n{}", failures.join("\n"));
}

#[test]
fn the_endemic_packs_carry_no_patient_name_and_no_season_marker() {
    let Some(dir) = common::embla_dir() else { return common::skip("embla-cases not present"); };
    for id in common::ENDEMIC_SIX {
        let Some(json) = common::show(&dir, common::ENDEMIC_REF, id) else {
            return common::skip("endemic branch not present");
        };
        let name = serde_json::from_str::<serde_json::Value>(&json).unwrap()["patient"]["name"].as_str().unwrap().to_string();
        let pack = compile(&json, Source::of("embla-cases", common::ENDEMIC_REF, &json)).unwrap_or_else(|e| panic!("{}: {}", e.case_id, e.reason));
        let text = serde_json::to_string(&pack).unwrap().to_lowercase();
        // whole words of three letters or more — a two-letter particle is not a name
        for token in name.split_whitespace().map(str::to_lowercase).filter(|t| t.chars().count() >= 3) {
            let hit = text.match_indices(token.as_str()).any(|(i, _)| {
                let before = text[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
                let after = text[i + token.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
                before && after
            });
            assert!(!hit, "{id}: the pack carries the name token {token:?}");
        }
        for marker in ["osce-", "Somsri", "Somchai", "/img/", "/clip/"] {
            assert!(!text.contains(marker), "{id}: the pack carries {marker:?}");
        }
    }
}

#[test]
fn the_dengue_child_narrows_her_pulse_pressure_before_the_systolic_falls() {
    let Some(dir) = common::embla_dir() else { return common::skip("embla-cases not present"); };
    let id = "embla-dengue-shock-syndrome-child-intern";
    let Some(json) = common::show(&dir, common::ENDEMIC_REF, id) else { return common::skip("endemic branch not present"); };
    let pack = compile(&json, Source::of("embla-cases", common::ENDEMIC_REF, &json)).unwrap_or_else(|e| panic!("{}", e.reason));
    let sce = vitals_sce::Sce::from_json(&pack.sce.to_string()).unwrap();
    let mut st = vitals_sce::SceState::new(sce);
    let pp0 = st.vitals.sbp - st.vitals.dbp;
    for _ in 0..180 {
        st.tick(1.0);
    }
    let pp = st.vitals.sbp - st.vitals.dbp;
    assert!(pp < pp0, "pulse pressure narrows: {pp0} → {pp}");
    assert!(st.vitals.sbp > 60.0, "still compensated at three minutes: {}", st.vitals.sbp);
}
