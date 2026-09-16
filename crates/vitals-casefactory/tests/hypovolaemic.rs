//! Adult hypovolaemic (non-haemorrhagic) shock: fluid in two phases is the therapy; the
//! electrolytes and the antibiotic are adjuncts; oral rehydration once perfusing; untreated the
//! volume runs out.

use vitals_casefactory::{compile, Source};
use vitals_replay::{replay, Step};

const CHOLERA: &str = include_str!("fixtures/synthetic-cholera-adult.json");

fn pack() -> vitals_casefactory::Pack {
    compile(CHOLERA, Source::of("embla-cases", "test", CHOLERA)).unwrap_or_else(|e| panic!("{}", e.reason))
}

#[test]
fn an_adult_with_cholera_shock_compiles_under_hypovolaemic_shock_and_dies_of_volume_untreated() {
    let p = pack();
    assert_eq!(p.archetype, "hypovolaemic_shock");
    assert!((360.0..=1440.0).contains(&p.replay.untreated_death_sec), "{}", p.replay.untreated_death_sec);
    assert_eq!(p.replay.win_outcome, "win_icu");
}

#[test]
fn the_win_needs_both_fluid_phases_and_prices_the_electrolytes_and_the_antibiotic() {
    let p = pack();
    let turn = p.sce["states"][0]["transitions"].as_array().unwrap().iter()
        .filter_map(|t| t.get("to_state").map(|_| t["when"].to_string())).next().unwrap();
    assert!(turn.contains("tx_fluids_rapid") && turn.contains("tx_fluids_second"), "{turn}");
    let path: Vec<&str> = p.replay.win_path.iter().map(|x| x.id.as_str()).collect();
    let at = |id: &str| path.iter().position(|x| *x == id).unwrap_or_else(|| panic!("{id} not on path {path:?}"));
    assert!(at("tx_fluids_rapid") < at("tx_fluids_second"));
    assert!(path.contains(&"tx_electrolytes") && path.contains(&"tx_antibiotics") && path.contains(&"tx_ors"));
    let paid: Vec<&str> = p.rubric["items"].as_array().unwrap().iter().filter_map(|i| i["needle"].as_str()).collect();
    assert!(paid.contains(&"tx_fluids_rapid") && paid.contains(&"tx_electrolytes"), "{paid:?}");
}

#[test]
fn oral_rehydration_before_the_drip_is_harm_and_an_antimotility_drug_is_harm() {
    let p = pack();
    let sce = p.sce.to_string();
    let r = replay(&sce, &[Step::Tick(10.0), Step::Act { text: "ors".into(), id: "tx_ors".into() }, Step::Tick(1.0)]).unwrap();
    assert!(r.harm_events.iter().any(|h| h.contains("cannot drink") || h.contains("vein")), "{:?}", r.harm_events);
    let r = replay(&sce, &[Step::Tick(10.0), Step::Act { text: "loperamide".into(), id: "tx_antimotility".into() }, Step::Tick(1.0)]).unwrap();
    assert!(r.harm_events.iter().any(|h| h.contains("antimotility")), "{:?}", r.harm_events);
    // and once the drip is in, the ORS is right
    let r = replay(&sce, &[Step::Tick(10.0), Step::Act { text: "ringer".into(), id: "tx_fluids_rapid".into() }, Step::Tick(30.0), Step::Act { text: "ors".into(), id: "tx_ors".into() }, Step::Tick(1.0)]).unwrap();
    assert!(r.harm_events.is_empty(), "{:?}", r.harm_events);
}

#[test]
fn a_child_with_cholera_still_compiles_under_the_paediatric_shape() {
    let mut v: serde_json::Value = serde_json::from_str(CHOLERA).unwrap();
    v["meta"]["id"] = serde_json::json!("synthetic-cholera-child-test");
    v["patient"]["age"] = serde_json::json!(6);
    v["exam_findings"][0]["value"] = serde_json::json!("80/64 mmHg");
    v["exam_findings"][1]["value"] = serde_json::json!("150/min");
    let s = v.to_string();
    let p = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_or_else(|e| panic!("{}", e.reason));
    assert_eq!(p.archetype, "paediatric_compensated_shock");
}
