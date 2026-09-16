//! Roles the authors found missing: the specific therapy, the transfusion and the isolation gate
//! in respiratory failure; antibiotics and dextrose as scored steps in a child's shock.

mod common;

use vitals_casefactory::{compile, Source};

const SYNTHETIC: &str = include_str!("fixtures/synthetic-septic-shock.json");

fn case(dx: &str, tags: &[&str], age: u32, vitals: &[&str], plan: &[&str], red: &[&str]) -> String {
    let mut v: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    v["meta"]["id"] = serde_json::json!(format!("synthetic-{}", dx.split_whitespace().next().unwrap().to_lowercase()));
    v["meta"]["title"] = serde_json::json!("test");
    v["patient"]["age"] = serde_json::json!(age);
    v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": dx, "aliases": [] });
    v["meta"]["search_tags"] = serde_json::json!(tags);
    v["hidden"]["red_flags"] = serde_json::json!(red);
    v["hidden"]["management_plan"] = serde_json::json!(plan);
    for (i, val) in vitals.iter().enumerate() {
        v["exam_findings"][i]["value"] = serde_json::json!(val);
    }
    v.to_string()
}

fn critical_ids(pack: &vitals_casefactory::Pack) -> Vec<String> {
    pack.sce["states"][0]["transitions"].as_array().unwrap().iter()
        .filter_map(|t| t.get("to_state").map(|_| t["when"].to_string()))
        .next().unwrap_or_default()
        .split('"').filter(|s| s.starts_with("tx_")).map(str::to_string).collect()
}

#[test]
fn diphtheria_is_won_by_antitoxin_and_the_airway_not_by_an_antibiotic_alone() {
    let s = case("Respiratory diphtheria with critical upper airway obstruction", &["diphtheria", "airway obstruction"], 7,
        &["100/60 mmHg", "140/min", "40/min", "88% on room air"],
        &["Isolate the child with droplet precautions before the examination",
          "Diphtheria antitoxin now, after a test dose, without waiting for the culture",
          "Secure the airway early: call the anaesthetist, tracheostomy set at the bedside, intubate before complete obstruction",
          "Penicillin or erythromycin for 14 days", "Oxygen by face mask", "Admit to intensive care"],
        &["Bull neck and stridor = the airway can close within hours — antitoxin and airway first"]);
    let pack = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_or_else(|e| panic!("{}", e.reason));
    assert_eq!(pack.archetype, "hypoxic_respiratory_failure");
    let crit = critical_ids(&pack);
    for want in ["tx_specific_therapy", "tx_airway", "tx_antibiotics", "tx_oxygen"] {
        assert!(crit.contains(&want.to_string()), "turn lacks {want}: {crit:?}");
    }
    let path: Vec<&str> = pack.replay.win_path.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(path[0], "tx_isolate", "the gate first: {path:?}");
    assert!(path.contains(&"tx_specific_therapy"));
    // skipping the gate is harm, priced by the rubric
    let harms: Vec<&str> = pack.rubric["items"].as_array().unwrap().iter().filter(|i| i["type"] == "no_harm").map(|i| i["needle"].as_str().unwrap()).collect();
    assert!(harms.iter().any(|h| h.contains("before isolation")), "{harms:?}");
}

#[test]
fn acute_chest_syndrome_is_won_by_the_transfusion_when_the_plan_names_it() {
    let s = case("Acute chest syndrome with hypoxaemia in sickle cell disease", &["sickle"], 19,
        &["110/70 mmHg", "118/min", "30/min", "87% on room air"],
        &["Oxygen to keep saturation above 95%", "Ceftriaxone and azithromycin", "Simple transfusion of packed red cells now (Hb 6.1, baseline 8)", "Incentive spirometry", "Admit"],
        &["Hb 6.1 with multilobar infiltrates = transfuse now"]);
    let pack = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_or_else(|e| panic!("{}", e.reason));
    let crit = critical_ids(&pack);
    assert!(crit.contains(&"tx_transfusion".to_string()), "{crit:?}");
    assert!(pack.replay.win_path.iter().any(|p| p.id == "tx_transfusion"));
}

#[test]
fn a_malnourished_child_in_shock_is_won_by_fluid_glucose_and_antibiotics_together() {
    let s = case("Severe acute malnutrition with septic shock and hypoglycaemia", &["sam", "child", "shock"], 2,
        &["70/50 mmHg", "170/min", "50/min", "94%"],
        &["Cautious measured fluid: 15 mL/kg over 1 hour, reassess", "10% dextrose 5 mL/kg at once for the glucose of 2.1", "Broad-spectrum antibiotics: ampicillin and gentamicin", "Rewarm the child", "Admit to the stabilisation ward"],
        &["Cold hands and a weak pulse in a wasted child = shock"]);
    let pack = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_or_else(|e| panic!("{}", e.reason));
    assert_eq!(pack.archetype, "paediatric_compensated_shock");
    let crit = critical_ids(&pack);
    for want in ["tx_fluids", "tx_dextrose", "tx_antibiotics"] {
        assert!(crit.contains(&want.to_string()), "turn lacks {want}: {crit:?}");
    }
    let paid: Vec<&str> = pack.rubric["items"].as_array().unwrap().iter().filter_map(|i| i["needle"].as_str()).collect();
    assert!(paid.contains(&"tx_dextrose") && paid.contains(&"tx_antibiotics"), "{paid:?}");
}

#[test]
fn the_world_lanes_yem_sen_tcd_packs_carry_the_new_roles_on_their_win_paths() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../embla-cases-world");
    if !root.join("cases").is_dir() { return common::skip("embla-cases-world not present"); }
    for (id, want) in [
        ("embla-respiratory-diphtheria-airway-obstruction-child-resident", "tx_specific_therapy"),
        ("embla-sickle-cell-acute-chest-syndrome-hypoxaemia-intern", "tx_transfusion"),
        ("embla-severe-acute-malnutrition-septic-shock-child-resident", "tx_antibiotics"),
    ] {
        let Ok(json) = std::fs::read_to_string(root.join("cases").join(id).join("case.json")) else { continue };
        let pack = compile(&json, Source::of("embla-cases", "worktree", &json)).unwrap_or_else(|e| panic!("{id}: {}", e.reason));
        let crit = critical_ids(&pack);
        assert!(crit.contains(&want.to_string()), "{id}: turn lacks {want}: {crit:?}");
        eprintln!("{id}: turn = {crit:?}");
    }
}
