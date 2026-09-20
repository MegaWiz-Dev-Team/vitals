//! The clinical advisor's per-case emphasis of 20 Sep 2026, audited against the packs: what a
//! case's own `management_safety` checklist names is paid even when it does not turn the
//! trajectory, and the harms the cases already forbid are priced — dexamethasone beside a
//! hydrocortisone that is allowed, the anticholinesterase and suxamethonium a taipan bite
//! forbids, the rapid bolus a child's shock and a coma forbid, full anticoagulation where only
//! prophylaxis is allowed, the ligature released too early, and the drugs the roles were missing
//! (tranexamic acid beside the uterotonics, vitamin A in measles, penicillin in a failing
//! rheumatic heart).

mod common;

use vitals_casefactory::{compile, Source};
use vitals_replay::Step;
use vitals_sce::{Sce, SceState};

const SYNTHETIC: &str = include_str!("fixtures/synthetic-septic-shock.json");

#[allow(clippy::too_many_arguments)]
fn case(id: &str, dx: &str, tags: &[&str], age: u32, vitals: &[&str], plan: &[&str], red: &[&str], criteria: &[&str]) -> String {
    case_with_harms(id, dx, tags, age, vitals, plan, red, criteria, &[])
}

#[allow(clippy::too_many_arguments)]
fn case_with_harms(id: &str, dx: &str, tags: &[&str], age: u32, vitals: &[&str], plan: &[&str], red: &[&str], criteria: &[&str], harm_criteria: &[&str]) -> String {
    let mut v: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    v["meta"]["id"] = serde_json::json!(id);
    v["meta"]["title"] = serde_json::json!("test");
    v["patient"]["age"] = serde_json::json!(age);
    v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": dx, "aliases": [] });
    v["meta"]["search_tags"] = serde_json::json!(tags);
    v["hidden"]["red_flags"] = serde_json::json!(red);
    v["hidden"]["management_plan"] = serde_json::json!(plan);
    for (i, val) in vitals.iter().enumerate() {
        v["exam_findings"][i]["value"] = serde_json::json!(val);
    }
    let dims = v["hidden"]["rubric"]["dimensions"].as_array_mut().unwrap();
    if !criteria.is_empty() {
        let d = dims.iter_mut().find(|d| d["key"] == "management_safety").unwrap();
        d["criteria"] = serde_json::json!(criteria);
    }
    if !harm_criteria.is_empty() {
        let d = dims.iter_mut().find(|d| d["key"] == "red_flag_recognition").unwrap();
        d["criteria"] = serde_json::json!(harm_criteria);
    }
    v.to_string()
}

fn pack_of(s: &str) -> vitals_casefactory::Pack {
    compile(s, Source::of("embla-cases", "test", s)).unwrap_or_else(|e| panic!("{}: {}", e.case_id, e.reason))
}

/// Every order the sheet pays for, with the label it pays under — `action`, `action_by`, and
/// each member of an `action_any`.
fn paid(pack: &vitals_casefactory::Pack) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for i in pack.rubric["items"].as_array().unwrap() {
        let label = i["label"].as_str().unwrap_or_default().to_string();
        match i["type"].as_str() {
            Some("action") | Some("action_by") => out.push((i["needle"].as_str().unwrap().to_string(), label)),
            Some("action_any") => out.extend(i["any_of"].as_array().unwrap().iter().map(|n| (n.as_str().unwrap().to_string(), label.clone()))),
            _ => {}
        }
    }
    out
}

fn no_harm(pack: &vitals_casefactory::Pack) -> Vec<String> {
    pack.rubric["items"].as_array().unwrap().iter()
        .filter(|i| i["type"] == "no_harm")
        .map(|i| i["needle"].as_str().unwrap().to_string())
        .collect()
}

fn ids(pack: &vitals_casefactory::Pack) -> Vec<String> {
    pack.sce["interventions"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

fn harmful(pack: &vitals_casefactory::Pack) -> Vec<String> {
    pack.sce["interventions"].as_array().unwrap().iter().filter(|i| i.get("harm").is_some()).map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

/// Type an order at `t` seconds and read the harm events.
fn typed(pack: &vitals_casefactory::Pack, before: &[&str], text: &str) -> Vec<String> {
    let mut steps = vec![Step::Tick(5.0)];
    for id in before {
        steps.push(Step::Act { text: id.to_string(), id: id.to_string() });
    }
    let id = matched(pack, text);
    assert!(!id.is_empty(), "{text:?} matches no order");
    steps.push(Step::Act { text: text.to_string(), id });
    steps.push(Step::Tick(1.0));
    let (_, r) = vitals_replay::resume(&pack.sce.to_string(), &steps).unwrap();
    r.harm_events
}

/// The order the engine's own matcher resolves free text to — first hit in the pack's order.
fn matched(pack: &vitals_casefactory::Pack, text: &str) -> String {
    let sce = Sce::from_json(&pack.sce.to_string()).unwrap();
    SceState::new(sce).resolve(text).unwrap_or_default()
}

// ── the case's own checklist pays for what the turn does not require ────────────────

#[test]
fn a_management_safety_criterion_pays_a_supportive_role_the_plan_names() {
    let s = case("synthetic-criterion", "Cerebral malaria with hypoglycaemia", &["malaria"], 30,
        &["96/58 mmHg", "118/min", "24/min", "95%", "39.1 °C", "GCS 9 (E2 V3 M4)"],
        &["50% dextrose 50 mL IV at once", "IV artesunate 2.4 mg/kg at 0, 12 and 24 hours",
          "After blood cultures: empirical ceftriaxone 2 g IV once daily", "If hypovolaemic give 500 mL isotonic crystalloid over 30 minutes and reassess"],
        &[],
        &["Empirical ceftriaxone 2 g IV after blood cultures", "Crystalloid 500 mL over 30 minutes, then reassess"]);
    let pack = pack_of(&s);
    assert_eq!(pack.archetype, "cns_depression_hypoglycaemia");
    let p = paid(&pack);
    assert!(p.iter().any(|(n, l)| n == "tx_antibiotics" && l.starts_with("Empirical ceftriaxone")), "{p:?}");
    assert!(p.iter().any(|(n, l)| n == "tx_fluids" && l.starts_with("Crystalloid 500 mL")), "{p:?}");
    // the turn is still the dextrose and the artesunate — the checklist pays, it does not require
    let turn = pack.sce["states"][0]["transitions"][1]["when"].to_string();
    assert!(!turn.contains("tx_antibiotics"), "{turn}");
    // and the golden path still earns the whole sheet
    assert_eq!(pack.replay.golden_score.earned, pack.replay.golden_score.max);
    assert_eq!(pack.criteria.len(), 2);
    assert_eq!(pack.criteria[0].interventions, vec!["tx_antibiotics".to_string()]);
}

#[test]
fn a_criterion_naming_a_role_already_paid_adds_no_second_item_and_an_unplaceable_one_is_listed() {
    let s = case("synthetic-criterion-dup", "Septic shock from an ascending urinary tract infection", &["sepsis"], 41,
        &["82/50 mmHg", "130/min", "28/min", "94%"],
        &["Two blood cultures, then broad-spectrum IV antibiotics (ceftriaxone 2 g) within the first hour",
          "Fluids: 30 mL/kg balanced crystalloid in boluses of 500 mL", "Start noradrenaline if MAP stays below 65 mmHg"],
        &[],
        &["Ceftriaxone 2 g IV within the first hour", "Vitamin A 200,000 IU by mouth"]);
    let pack = pack_of(&s);
    let p = paid(&pack);
    assert_eq!(p.iter().filter(|(n, _)| n == "tx_antibiotics").count(), 1, "{p:?}");
    assert_eq!(pack.criteria.len(), 2);
    assert!(pack.criteria[1].interventions.is_empty(), "vitamin A has no order in septic shock: {:?}", pack.criteria[1]);
}

// ── the harms the cases forbid, priced ──────────────────────────────────────────────

#[test]
fn dexamethasone_is_a_priced_harm_while_hydrocortisone_stays_the_therapy_for_refractory_shock() {
    let s = case("synthetic-dexa", "Meningococcal meningitis with septic shock", &["meningitis"], 19,
        &["78/40 mmHg", "132/min", "30/min", "95%"],
        &["Ceftriaxone 2 g IV within the hour after blood cultures", "Crystalloid 30 mL/kg in boluses", "Noradrenaline if MAP stays below 65",
          "If shock persists on noradrenaline for 4 hours: hydrocortisone 200 mg/day IV for refractory septic shock; do not give dexamethasone for the meningitis itself"],
        &["Do not give dexamethasone as adjunctive therapy for meningitis in this setting"], &[]);
    let pack = pack_of(&s);
    let h = harmful(&pack);
    assert!(h.contains(&"tx_dexamethasone".to_string()), "{h:?}");
    assert!(ids(&pack).contains(&"tx_steroids".to_string()) && !h.contains(&"tx_steroids".to_string()), "hydrocortisone is still an order: {:?}", ids(&pack));
    assert_eq!(matched(&pack, "dexamethasone 10 mg IV"), "tx_dexamethasone");
    assert_eq!(matched(&pack, "hydrocortisone 50 mg IV"), "tx_steroids");
    assert!(!typed(&pack, &[], "dexamethasone 10 mg IV").is_empty());
    assert!(typed(&pack, &[], "hydrocortisone 50 mg IV").is_empty());
    assert!(no_harm(&pack).iter().any(|n| n.contains("dexamethasone")), "{:?}", no_harm(&pack));
}

#[test]
fn the_anticholinesterase_and_suxamethonium_are_priced_harms_where_a_taipan_bite_forbids_them() {
    let s = case("synthetic-taipan", "Papuan taipan envenoming with paralysis and coagulopathy", &["envenoming", "taipan"], 44,
        &["126/80 mmHg", "104/min", "26/min", "91% on room air"],
        &["Secure the airway now by elective intubation: rapid-sequence induction with ketamine and rocuronium (suxamethonium is avoided because of the rhabdomyolysis and rising potassium)",
          "Taipan antivenom 1 vial IV as soon as systemic envenoming is recognised",
          "Do not rely on an anticholinesterase test: taipan paralysis is presynaptic and will not respond"],
        &["Presynaptic paralysis does not reverse with antivenom or neostigmine"], &[]);
    let pack = pack_of(&s);
    assert_eq!(pack.archetype, "neuromuscular_respiratory_failure");
    let h = harmful(&pack);
    assert!(h.contains(&"tx_neostigmine".to_string()) && h.contains(&"tx_succinylcholine".to_string()), "{h:?}");
    assert!(!typed(&pack, &[], "neostigmine 1.5 mg IV").is_empty());
    assert!(!typed(&pack, &[], "suxamethonium 100 mg for RSI").is_empty());
    let nh = no_harm(&pack);
    assert!(nh.iter().any(|n| n.contains("neostigmine") || n.contains("anticholinesterase")), "{nh:?}");
    assert!(nh.iter().any(|n| n.contains("suxamethonium") || n.contains("succinylcholine")), "{nh:?}");
}

#[test]
fn a_krait_bite_that_allows_the_neostigmine_trial_keeps_it_as_an_order() {
    let s = case("synthetic-krait", "Common krait envenoming with neuromuscular respiratory failure", &["envenoming", "krait"], 34,
        &["118/76 mmHg", "110/min", "26/min", "90% on room air"],
        &["Airway first: intubate and ventilate", "Indian polyvalent anti-snake venom 10 vials over 30-60 minutes",
          "Atropine-neostigmine trial: atropine 0.6 mg IV followed by neostigmine 1.5 mg IV; if no objective improvement, stop"],
        &["Presynaptic toxin means antivenom and neostigmine cannot undo paralysis already established"], &[]);
    let pack = pack_of(&s);
    assert!(ids(&pack).contains(&"tx_neostigmine".to_string()));
    assert!(!harmful(&pack).contains(&"tx_neostigmine".to_string()), "{:?}", harmful(&pack));
    assert!(typed(&pack, &[], "neostigmine 1.5 mg IV").is_empty());
}

#[test]
fn a_rapid_fluid_bolus_is_a_priced_harm_in_a_childs_shock_and_in_a_coma_when_the_case_forbids_it() {
    let sam = case("synthetic-sam", "Severe acute malnutrition with shock", &["malnutrition", "child", "shock"], 2,
        &["70/50 mmHg", "172/min", "50/min", "94%"],
        &["Ringer's lactate with 5% dextrose 15 mL/kg IV over 1 hour, reassessed every 10 minutes", "10% dextrose 5 mL/kg at once",
          "Ampicillin and gentamicin", "Do not give a 20 mL/kg fluid bolus, do not give diuretics for the oedema"],
        &["A 20 mL/kg bolus can kill him by heart failure"], &[]);
    let pack = pack_of(&sam);
    assert_eq!(pack.archetype, "paediatric_compensated_shock");
    assert!(harmful(&pack).contains(&"tx_rapid_bolus".to_string()), "{:?}", harmful(&pack));
    assert_eq!(matched(&pack, "fluid bolus 20 ml/kg"), "tx_rapid_bolus");
    assert_eq!(matched(&pack, "ringer's lactate 15 ml/kg over 1 hour"), "tx_fluids");
    assert!(!typed(&pack, &[], "20 ml/kg saline bolus").is_empty());
    assert!(typed(&pack, &[], "ringer's lactate 15 ml/kg over 1 hour").is_empty());
    assert!(no_harm(&pack).iter().any(|n| n.contains("bolus")), "{:?}", no_harm(&pack));

    let coma = case("synthetic-coma-bolus", "Cerebral malaria with hypoglycaemia and severe anaemia", &["malaria", "child"], 4,
        &["92/56 mmHg", "150/min", "44/min", "93%", "39.4 °C", "GCS 8 (E2 V2 M4)"],
        &["10% dextrose 5 mL/kg IV over 5-10 minutes", "IV artesunate 3 mg/kg at 0, 12 and 24 hours",
          "Fluids: no rapid bolus (FEAST) — give maintenance isotonic fluid with 10% dextrose", "Transfuse whole blood 20 mL/kg over 3-4 hours"],
        &["Do not give a rapid fluid bolus to this child"], &[]);
    let pack = pack_of(&coma);
    assert_eq!(pack.archetype, "cns_depression_hypoglycaemia");
    assert!(harmful(&pack).contains(&"tx_rapid_bolus".to_string()), "{:?}", harmful(&pack));
    // the transfusion's own 20 mL/kg is not a bolus
    assert_eq!(matched(&pack, "whole blood 20 ml/kg"), "tx_transfusion");
    assert!(typed(&pack, &[], "whole blood 20 ml/kg").is_empty());

    // and where the bolus is the therapy, the same sentence forbids nothing
    let septic = case("synthetic-septic-bolus", "Septic shock from an ascending urinary tract infection", &["sepsis"], 41,
        &["82/50 mmHg", "130/min", "28/min", "94%"],
        &["Broad-spectrum antibiotics after cultures", "Fluids: 30 mL/kg crystalloid in boluses of 500 mL; do not give a rapid large bolus without reassessing"],
        &[], &[]);
    let pack = pack_of(&septic);
    assert!(!ids(&pack).contains(&"tx_rapid_bolus".to_string()), "{:?}", ids(&pack));
}

#[test]
fn full_anticoagulation_is_a_priced_harm_where_the_case_allows_only_prophylaxis() {
    let s = case("synthetic-chagas", "Acute Chagas myocarditis with cardiogenic shock", &["chagas", "myocarditis"], 27,
        &["84/60 mmHg", "118/min", "28/min", "91%"],
        &["Dobutamine 5 µg/kg/min; add noradrenaline if MAP stays below 65", "Benznidazole once stable",
          "Thromboprophylaxis with low-molecular-weight heparin once bleeding risk is acceptable; no routine full anticoagulation unless thrombus or atrial fibrillation"],
        &["Pericardial effusion — full anticoagulation risks tamponade"], &[]);
    let pack = pack_of(&s);
    assert_eq!(pack.archetype, "cardiogenic_shock");
    assert!(harmful(&pack).contains(&"tx_full_anticoagulation".to_string()), "{:?}", harmful(&pack));
    assert!(ids(&pack).contains(&"tx_anticoagulation".to_string()));
    assert_eq!(matched(&pack, "therapeutic enoxaparin 1 mg/kg twice daily"), "tx_full_anticoagulation");
    assert_eq!(matched(&pack, "enoxaparin 40 mg once daily prophylaxis"), "tx_anticoagulation");
    assert!(!typed(&pack, &[], "full anticoagulation with heparin infusion").is_empty());
    assert!(typed(&pack, &[], "enoxaparin 40 mg once daily prophylaxis").is_empty());
}

#[test]
fn prednisolone_is_a_steroid_harm_where_the_case_forbids_steroids_in_shock() {
    let s = case("synthetic-rhd", "Rheumatic heart disease with acute heart failure and low output", &["rheumatic", "heart failure"], 24,
        &["86/64 mmHg", "124/min", "30/min", "90%"],
        &["Dobutamine 5 µg/kg/min; add noradrenaline if MAP stays below 65", "Furosemide 40 mg IV once the pressure is supported",
          "After three blood cultures, benzathine benzylpenicillin 1.2 million units IM once"],
        &["No routine steroids while in shock"], &[]);
    let pack = pack_of(&s);
    assert!(harmful(&pack).contains(&"tx_steroids".to_string()), "{:?}", harmful(&pack));
    assert_eq!(matched(&pack, "prednisolone 60 mg"), "tx_steroids");
    assert!(!typed(&pack, &[], "prednisolone 60 mg").is_empty());
    // and the penicillin is an order in a failing heart
    assert!(ids(&pack).contains(&"tx_antibiotics".to_string()), "{:?}", ids(&pack));
    assert_eq!(matched(&pack, "benzathine penicillin 1.2 million units IM"), "tx_antibiotics");
}

#[test]
fn the_red_flag_checklist_puts_the_harm_it_names_at_the_head_of_a_full_sheet() {
    // six harms the case forbids and five points to price them: without the checklist the
    // bolus, listed last, falls off the sheet
    let plan = &["50% dextrose 50 mL IV at once", "IV artesunate 2.4 mg/kg at 0, 12 and 24 hours",
        "If hypovolaemic give 500 mL isotonic crystalloid over 30 minutes and reassess (no rapid large boluses)",
        "No NSAIDs; no corticosteroids or mannitol; no oral antimalarials alone while vomiting; no prophylactic phenobarbital; no sedatives before the airway"];
    let vitals = &["96/58 mmHg", "118/min", "24/min", "95%", "39.1 °C", "GCS 9 (E2 V3 M4)"];
    let without = case("synthetic-bolus-order", "Cerebral malaria with hypoglycaemia", &["malaria"], 30, vitals, plan, &[], &[]);
    let pack = pack_of(&without);
    assert!(harmful(&pack).contains(&"tx_rapid_bolus".to_string()), "{:?}", harmful(&pack));
    assert!(!no_harm(&pack).iter().any(|n| n.contains("bolus")), "the sheet is full and the bolus is last: {:?}", no_harm(&pack));
    let with = case_with_harms("synthetic-bolus-order", "Cerebral malaria with hypoglycaemia", &["malaria"], 30, vitals, plan, &[], &[],
        &["No aggressive or rapid fluid bolus"]);
    let pack = pack_of(&with);
    let nh = no_harm(&pack);
    assert!(nh[0].contains("bolus"), "{nh:?}");
    assert_eq!(pack.replay.golden_score.earned, pack.replay.golden_score.max);
}

#[test]
fn releasing_the_ligature_before_the_antivenom_and_the_airway_is_a_priced_harm() {
    let s = case_with_harms("synthetic-ligature", "Common krait envenoming with neuromuscular respiratory failure", &["envenoming", "krait"], 34,
        &["118/76 mmHg", "110/min", "26/min", "90% on room air"],
        &["Airway first: intubate and ventilate", "Indian polyvalent anti-snake venom 10 vials over 30-60 minutes",
          "Release the ankle ligature slowly only after antivenom is running and the airway is secured",
          "No NSAIDs or intramuscular injections; no incision or suction at the bite; no sedatives before the airway"],
        &["The ankle ligature must be released only after antivenom is running and the airway is secured — sudden release can precipitate deterioration"],
        &["Release the tourniquet only after the airway is secured and antivenom has started"],
        &["Do not release the tourniquet before the airway and the antivenom"]);
    let pack = pack_of(&s);
    assert!(!typed(&pack, &[], "release the tourniquet").is_empty(), "released first: harm");
    assert!(!typed(&pack, &["tx_airway"], "release the tourniquet").is_empty(), "released before the antivenom: harm");
    assert!(typed(&pack, &["tx_airway", "tx_specific_therapy"], "release the tourniquet").is_empty(), "released after both: right");
    assert!(no_harm(&pack).iter().any(|n| n.contains("ligature")), "{:?}", no_harm(&pack));
    assert!(paid(&pack).iter().any(|(n, _)| n == "tx_ligature"), "{:?}", paid(&pack));
    assert_eq!(pack.replay.golden_score.earned, pack.replay.golden_score.max);
}

#[test]
fn tranexamic_acid_is_its_own_order_beside_the_uterotonics_and_the_criterion_pays_it() {
    let s = case("synthetic-pph", "Postpartum haemorrhage from uterine atony with haemorrhagic shock", &["postpartum", "haemorrhage"], 23,
        &["78/46 mmHg", "136/min", "28/min", "96%"],
        &["Two large-bore lines, crystalloid 1 L fast", "Uterotonic within minutes: oxytocin 10 units IV, then ergometrine 0.2 mg IM, then misoprostol 800 µg sublingual; bimanual uterine massage",
          "Tranexamic acid 1 g IV over 10 minutes now, within 3 hours of birth", "Transfuse O-negative then crossmatched blood"],
        &[], &["Tranexamic acid 1 g IV within 3 hours of birth"]);
    let pack = pack_of(&s);
    assert_eq!(pack.archetype, "haemorrhagic_shock");
    assert!(ids(&pack).contains(&"tx_tranexamic".to_string()), "{:?}", ids(&pack));
    assert_eq!(matched(&pack, "tranexamic acid 1 g IV"), "tx_tranexamic");
    assert_eq!(matched(&pack, "oxytocin 10 units IV"), "tx_haemostasis");
    let turn = pack.sce["states"][0]["transitions"][1]["when"].to_string();
    assert!(turn.contains("tx_haemostasis") && !turn.contains("tx_tranexamic"), "{turn}");
    assert!(paid(&pack).iter().any(|(n, _)| n == "tx_tranexamic"), "{:?}", paid(&pack));
}

#[test]
fn vitamin_a_is_an_order_in_measles_and_the_criterion_pays_it() {
    let s = case("synthetic-measles", "Measles with severe pneumonia and hypoxaemia", &["measles", "child"], 2,
        &["92/58 mmHg", "150/min", "56/min", "86% on room air"],
        &["Isolate with airborne precautions before the examination", "Oxygen by nasal prongs to keep SpO2 at or above 90%",
          "Ampicillin 50 mg/kg IV every 6 hours plus gentamicin 7.5 mg/kg once daily", "Vitamin A 200,000 IU by mouth today and again tomorrow"],
        &[], &["Vitamin A 200,000 IU on day 1 and day 2"]);
    let pack = pack_of(&s);
    assert_eq!(pack.archetype, "hypoxic_respiratory_failure");
    assert!(ids(&pack).contains(&"tx_vitamin_a".to_string()), "{:?}", ids(&pack));
    assert_eq!(matched(&pack, "vitamin a 200,000 IU"), "tx_vitamin_a");
    assert!(paid(&pack).iter().any(|(n, l)| n == "tx_vitamin_a" && l.starts_with("Vitamin A")), "{:?}", paid(&pack));
    assert_eq!(pack.replay.golden_score.earned, pack.replay.golden_score.max);
}

// ── the eighteen, against the library ───────────────────────────────────────────────

#[test]
fn the_eighteen_endemic_packs_carry_the_advisors_items() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../embla-cases-world");
    if !root.join("cases").is_dir() { return common::skip("embla-cases-world not present"); }
    // (case, paid needles, harm needle fragments)
    let want: &[(&str, &[&str], &[&str])] = &[
        ("embla-severe-falciparum-malaria-cerebral-resident", &["tx_dextrose", "tx_specific_therapy", "tx_antibiotics", "tx_fluids"], &["bolus"]),
        ("embla-lassa-fever-haemorrhagic-shock-resident", &["tx_specific_therapy", "tx_isolate", "tx_electrolytes"], &["before isolation"]),
        ("embla-acute-chagas-myocarditis-cardiogenic-shock-resident", &["tx_inotrope", "tx_vasopressor", "tx_anticoagulation"], &["fluid bolus", "corticosteroid", "full anticoagulation"]),
        ("embla-typhoid-ileal-perforation-septic-shock-intern", &["tx_antibiotics", "tx_source_control"], &["fluoroquinolone"]),
        ("embla-neurotoxic-krait-envenoming-respiratory-failure-intern", &["tx_specific_therapy", "tx_airway", "tx_ligature"], &["ligature"]),
        ("embla-dengue-shock-syndrome-child-intern", &["tx_fluids"], &["non-steroidal"]),
        ("embla-meningococcal-meningitis-septic-shock-intern", &["tx_antibiotics", "tx_fluids", "tx_vasopressor"], &["dexamethasone"]),
        ("embla-cholera-severe-dehydration-shock-child-student", &["tx_fluids", "tx_antibiotics"], &[]),
        ("embla-measles-severe-pneumonia-hypoxaemia-child-student", &["tx_antibiotics", "tx_oxygen", "tx_vitamin_a"], &[]),
        ("embla-pneumocystis-pneumonia-advanced-hiv-respiratory-failure-intern", &["tx_antibiotics", "tx_steroids"], &[]),
        ("embla-papuan-taipan-envenoming-paralysis-coagulopathy-resident", &["tx_specific_therapy", "tx_airway"], &["anticholinesterase", "suxamethonium"]),
        ("embla-cerebral-malaria-child-hypoglycaemia-severe-anaemia-resident", &["tx_specific_therapy", "tx_dextrose", "tx_transfusion"], &["bolus"]),
        ("embla-severe-acute-malnutrition-septic-shock-child-resident", &["tx_fluids", "tx_dextrose", "tx_antibiotics"], &["bolus"]),
        ("embla-rheumatic-heart-disease-acute-heart-failure-low-output-resident", &["tx_inotrope", "tx_diuretic", "tx_antibiotics"], &["corticosteroid"]),
        ("embla-respiratory-diphtheria-airway-obstruction-child-resident", &["tx_specific_therapy", "tx_airway"], &[]),
        ("embla-sickle-cell-acute-chest-syndrome-hypoxaemia-intern", &["tx_antibiotics", "tx_oxygen", "tx_transfusion"], &[]),
        ("embla-postpartum-haemorrhage-uterine-atony-home-birth-intern", &["tx_haemostasis", "tx_tranexamic"], &[]),
        ("embla-puerperal-sepsis-retained-products-septic-shock-intern", &["tx_fluids", "tx_antibiotics", "tx_source_control"], &[]),
    ];
    let mut failures = Vec::new();
    for (id, needles, harms) in want {
        let Ok(json) = std::fs::read_to_string(root.join("cases").join(id).join("case.json")) else { failures.push(format!("{id}: not in library")); continue };
        let pack = match compile(&json, Source::of("embla-cases", "worktree", &json)) {
            Ok(p) => p,
            Err(e) => { failures.push(format!("{id}: refused — {}", e.reason)); continue }
        };
        let p: Vec<String> = paid(&pack).into_iter().map(|(n, _)| n).collect();
        for n in *needles {
            if !p.contains(&n.to_string()) { failures.push(format!("{id}: {n} is not paid — paid {p:?}")); }
        }
        let nh = no_harm(&pack);
        for h in *harms {
            if !nh.iter().any(|n| n.contains(h)) { failures.push(format!("{id}: no priced harm mentions {h:?} — {nh:?}")); }
        }
        for c in &pack.criteria {
            if c.interventions.is_empty() { failures.push(format!("{id}: criterion placed nowhere: {:?}", c.step)); }
        }
        if pack.replay.golden_score.earned != pack.replay.golden_score.max {
            failures.push(format!("{id}: golden path {}/{}", pack.replay.golden_score.earned, pack.replay.golden_score.max));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_release_before_the_line_is_a_harm_the_engine_can_fire() {
    // the engine sees the branch harm as one it can fire, so the sheet's no_harm resolves
    let s = case("synthetic-ligature-2", "Common krait envenoming with neuromuscular respiratory failure", &["envenoming", "krait"], 34,
        &["118/76 mmHg", "110/min", "26/min", "90% on room air"],
        &["Intubate and ventilate", "Anti-snake venom 10 vials", "Release the ligature only after antivenom is running and the airway is secured"],
        &[], &[]);
    let pack = pack_of(&s);
    let sce = Sce::from_json(&pack.sce.to_string()).unwrap();
    let mut st = SceState::new(sce);
    st.tick(5.0);
    st.apply_id("tx_ligature");
    assert!(st.harm_events.iter().any(|h| h.contains("ligature")), "{:?}", st.harm_events);
}
