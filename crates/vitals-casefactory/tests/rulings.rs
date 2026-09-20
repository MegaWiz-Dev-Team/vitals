//! The clinical advisor's seven compiler rulings of 20 Sep 2026 (vitals
//! docs/internal/REVIEW_RESULT_CLINICAL_ADVISOR_2026-09-20.md §3):
//!
//! 3.1 the shift's pass mark is the ward's 70 %, whatever the case's own OSCE pass mark says;
//! 3.2 a converted SVT or AF goes home when it converted on drugs and the heart is structurally
//!     sound, and to intensive care when cardioversion was needed or the heart is not;
//! 3.3 cerebral malaria without the antimalarial dies at GCS 3 — already so, confirmed here;
//! 3.4 the ACLS scenarios are listed VF/pVT first, PEA/asystole second, bradycardia third;
//! 3.5 the defibrillator's shock, indicated and on time, is a rubric item the kit button earns;
//! 3.6 a child's shock pays for the antibiotics and the glucose beside the fluid — confirmed;
//! 3.7 a case whose presenting vitals are within normal is cut from the ward, by name.

mod common;

use vitals_casefactory::report::{render, Outcome};
use vitals_casefactory::{compile, Source};
use vitals_replay::Step;
use vitals_sce::{Sce, SceState};

const SEPTIC: &str = include_str!("fixtures/synthetic-septic-shock.json");
const VF: &str = include_str!("fixtures/synthetic-vf-arrest.json");

fn pack_of(s: &str) -> vitals_casefactory::Pack {
    compile(s, Source::of("embla-cases", "test", s)).unwrap_or_else(|e| panic!("{}: {}", e.case_id, e.reason))
}

fn library() -> Option<std::path::PathBuf> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../embla-cases-world");
    root.join("cases").is_dir().then_some(root)
}

fn library_pack(root: &std::path::Path, id: &str) -> Result<vitals_casefactory::Pack, String> {
    let json = std::fs::read_to_string(root.join("cases").join(id).join("case.json")).map_err(|e| format!("{id}: {e}"))?;
    compile(&json, Source::of("embla-cases", "worktree", &json)).map_err(|e| format!("{}: {}", e.case_id, e.reason))
}

/// A rhythm case built on the arrest fixture: the vitals, the diagnosis, the plan.
fn rhythm_case(id: &str, dx: &str, aliases: &[&str], vitals: &[&str], plan: &[&str], pmh: &[&str], age: u32) -> String {
    let mut v: serde_json::Value = serde_json::from_str(VF).unwrap();
    v["meta"]["id"] = serde_json::json!(id);
    v["meta"]["title"] = serde_json::json!("Synthetic test case: palpitations");
    v["meta"]["care_setting"] = serde_json::json!("ER");
    v["meta"]["clinical_tier"] = serde_json::json!(4);
    v["meta"]["search_tags"] = serde_json::json!(["synthetic", "test"]);
    v["patient"]["age"] = serde_json::json!(age);
    v["patient"]["name"] = serde_json::json!("Zuzanna Quix"); // the fixture's "Beta" would trip the name scan on "beta-blocker"
    v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": dx, "aliases": aliases });
    v["hidden"]["red_flags"] = serde_json::json!(["Palpitations with a narrow regular tachycardia — treat the rhythm"]);
    v["hidden"]["management_plan"] = serde_json::json!(plan);
    v["presentation"]["pmh"] = serde_json::json!(pmh.iter().map(|p| serde_json::json!({ "display": p })).collect::<Vec<_>>());
    for (i, val) in vitals.iter().enumerate() {
        v["exam_findings"][i]["value"] = serde_json::json!(val);
    }
    v.to_string()
}

fn svt(id: &str, sbp: &str, pmh: &[&str]) -> String {
    rhythm_case(id, "Paroxysmal supraventricular tachycardia", &["SVT", "PSVT"],
        &[sbp, "175/min, regular", "20/min", "98% on room air", "Anxious, GCS 15"],
        &["Vagal manoeuvre: modified Valsalva", "Adenosine 6 mg rapid IV push, then 12 mg if no response",
          "Synchronised cardioversion 50-100 J if unstable", "Observe on the monitor; discharge with cardiology follow-up if it converts and stays converted"],
        pmh, 28)
}

fn af(id: &str, pmh: &[&str], plan_extra: &str) -> String {
    let mut plan = vec!["Rate control: metoprolol 5 mg IV, repeated", "Anticoagulation: apixaban after the CHA2DS2-VASc score", "Synchronised cardioversion 120-200 J if unstable", "Admit to a monitored bed"];
    if !plan_extra.is_empty() { plan.push(plan_extra); }
    rhythm_case(id, "Atrial fibrillation with rapid ventricular response", &["AF with RVR", "atrial fibrillation"],
        &["128/80 mmHg", "138/min, irregularly irregular", "22/min", "96% on room air", "Sweaty, GCS 15"],
        &plan, pmh, 62)
}

/// Run the pack's own golden path and read the outcome.
fn run_path(pack: &vitals_casefactory::Pack, extra: &[(f64, &str)]) -> (Option<String>, Vec<String>) {
    let mut steps: Vec<(f64, String)> = pack.replay.win_path.iter().map(|p| (p.t_sec, p.id.clone())).collect();
    steps.extend(extra.iter().map(|(t, id)| (*t, id.to_string())));
    steps.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut tape = Vec::new();
    let mut t = 0.0;
    for (at, id) in steps {
        while t < at { tape.push(Step::Tick(1.0)); t += 1.0; }
        tape.push(Step::Act { text: id.clone(), id });
    }
    for _ in 0..(40 * 60) { tape.push(Step::Tick(1.0)); }
    let r = vitals_replay::replay(&pack.sce.to_string(), &tape).unwrap();
    (r.outcome, r.harm_events)
}

fn outcomes(pack: &vitals_casefactory::Pack) -> Vec<String> {
    pack.sce["outcomes"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap().to_string()).collect()
}

// ── 3.1 ───────────────────────────────────────────────────────────────────────────────

#[test]
fn r3_1_the_pass_mark_is_the_wards_seventy_percent_whatever_the_case_says() {
    let pack = pack_of(SEPTIC); // the fixture's own pass_mark is 60
    assert_eq!(pack.rubric["pass_bps"], 7000);
    assert_eq!(pack.replay.golden_score.pass_bps, 7000);
    let mut v: serde_json::Value = serde_json::from_str(SEPTIC).unwrap();
    v["hidden"]["rubric"]["pass_mark"] = serde_json::json!(50);
    let s = v.to_string();
    let pack = pack_of(&s);
    assert_eq!(pack.rubric["pass_bps"], 7000, "the case's 50 is the OSCE's bar, not the shift's");
    assert_eq!(pack.rubric["pass_bps"].as_u64().unwrap() as u32, vitals_progress::STAR_PASS_BPS, "one number for the whole system");
    assert!(pack.rubric["status"].as_str().unwrap().contains("70"), "{}", pack.rubric["status"]);
}

// ── 3.2 ───────────────────────────────────────────────────────────────────────────────

#[test]
fn r3_2_a_stable_svt_converted_on_drugs_goes_home_and_the_same_svt_cardioverted_goes_to_icu() {
    let pack = pack_of(&svt("synthetic-svt-stable", "108/70 mmHg", &[]));
    assert_eq!(pack.archetype, "acls_tachycardia_svt");
    let outs = outcomes(&pack);
    assert!(outs.contains(&"win_discharge".to_string()) && outs.contains(&"win_icu".to_string()), "{outs:?}");
    // the golden path (vagal, adenosine) converts and the patient goes home
    assert_eq!(pack.replay.win_outcome, "win_discharge");
    let (out, harm) = run_path(&pack, &[]);
    assert_eq!(out.as_deref(), Some("WinDischarge"), "{harm:?}");
    // the same patient cardioverted instead: intensive care
    let sce = Sce::from_json(&pack.sce.to_string()).unwrap();
    let mut st = SceState::new(sce);
    st.tick(20.0);
    st.apply_id("tx_cardioversion");
    for _ in 0..(20 * 60) { st.tick(1.0); if st.outcome().is_some() { break; } }
    assert_eq!(st.outcome_id(), Some("win_icu"));
    // and the sheet pays the survival either way
    let outcome_item = pack.rubric["items"].as_array().unwrap().iter().find(|i| i["type"] == "outcome").unwrap();
    let any: Vec<&str> = outcome_item["any_of"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(any.contains(&"win_discharge") && any.contains(&"win_icu"), "{any:?}");
}

#[test]
fn r3_2_an_unstable_svt_and_a_rhythm_on_a_structurally_diseased_heart_go_to_icu() {
    let unstable = pack_of(&svt("synthetic-svt-unstable", "82/50 mmHg", &[]));
    assert_eq!(unstable.replay.win_outcome, "win_icu", "unstable at the door: cardioversion is what it needed");
    let (out, _) = run_path(&unstable, &[]);
    assert_eq!(out.as_deref(), Some("WinIcu"));

    let structural = pack_of(&svt("synthetic-svt-structural", "108/70 mmHg", &["Rheumatic mitral stenosis"]));
    assert_eq!(structural.replay.win_outcome, "win_icu", "a structural heart disease in the history");
    let (out, _) = run_path(&structural, &[]);
    assert_eq!(out.as_deref(), Some("WinIcu"));

    // AF on a sound heart, rate-controlled and anticoagulated: home; on a cardiomyopathy: ICU
    let sound = pack_of(&af("synthetic-af-sound", &["Hyperthyroidism"], "Echocardiogram: no structural heart disease"));
    assert_eq!(sound.archetype, "acls_tachycardia_af");
    assert_eq!(sound.replay.win_outcome, "win_discharge");
    let sick = pack_of(&af("synthetic-af-structural", &["Dilated cardiomyopathy, LVEF 30%"], ""));
    assert_eq!(sick.replay.win_outcome, "win_icu");
}

#[test]
fn r3_2_the_library_rhythm_cases_end_where_the_ruling_sends_them() {
    let Some(root) = library() else { return common::skip("embla-cases-world not present") };
    // four SVTs on sound hearts, stable at the door: home on adenosine; two AFs with valve disease: ICU
    for (id, want) in [
        ("ddx-psvt-1-en", "win_discharge"), ("ddx-psvt-2-en", "win_discharge"), ("ddx-psvt-3-en", "win_discharge"), ("ddx-psvt-4-en", "win_discharge"),
        ("ddx-atrial-fibrillation-2-en", "win_icu"), ("ddx-atrial-fibrillation-4-en", "win_icu"),
    ] {
        let pack = library_pack(&root, id).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pack.replay.win_outcome, want, "{id}");
    }
}

// ── 3.3 ───────────────────────────────────────────────────────────────────────────────

#[test]
fn r3_3_cerebral_malaria_without_the_antimalarial_dies_at_gcs_3_already() {
    let Some(root) = library() else { return common::skip("embla-cases-world not present") };
    for id in ["embla-severe-falciparum-malaria-cerebral-resident", "embla-cerebral-malaria-child-hypoglycaemia-severe-anaemia-resident"] {
        let pack = library_pack(&root, id).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pack.archetype, "cns_depression_hypoglycaemia");
        // nobody acts: the consciousness runs down to 3 and the case ends there
        let sce = Sce::from_json(&pack.sce.to_string()).unwrap();
        let mut st = SceState::new(sce);
        for _ in 0..(30 * 60) { st.tick(1.0); if st.outcome().is_some() { break; } }
        assert_eq!(st.outcome_id(), Some("death_arrest"), "{id}");
        assert_eq!(st.vitals.gcs, 3, "{id}: GCS at death");
        // the sugar corrected but no antimalarial: the coma stops deepening, and the patient still dies —
        // of the airway and the pressure the parasites keep taking
        let sce = Sce::from_json(&pack.sce.to_string()).unwrap();
        let mut st = SceState::new(sce);
        st.tick(20.0);
        st.apply_id("tx_dextrose");
        for _ in 0..(40 * 60) { st.tick(1.0); if st.outcome().is_some() { break; } }
        assert_eq!(st.outcome_id(), Some("death_arrest"), "{id}: dextrose alone does not save a cerebral malaria");
        eprintln!("{id}: dextrose only — dies with GCS {}, SpO2 {:.0}, SBP {:.0}", st.vitals.gcs, st.vitals.spo2, st.vitals.sbp);
    }
}

// ── 3.4 ───────────────────────────────────────────────────────────────────────────────

#[test]
fn r3_4_the_report_lists_the_acls_scenarios_vf_first_then_pea_asystole_then_bradycardia() {
    let vf = pack_of(VF);
    let mut pea_v: serde_json::Value = serde_json::from_str(VF).unwrap();
    pea_v["meta"]["id"] = serde_json::json!("aaa-pea-first-by-name");
    pea_v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": "Cardiac arrest — pulseless electrical activity", "aliases": ["PEA arrest", "cardiac arrest"] });
    let pea = pack_of(&pea_v.to_string());
    let mut brady_v: serde_json::Value = serde_json::from_str(VF).unwrap();
    brady_v["meta"]["id"] = serde_json::json!("aab-brady-second-by-name");
    brady_v["meta"]["title"] = serde_json::json!("Synthetic test case: dizzy and grey with a pulse of 36");
    brady_v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": "Symptomatic bradycardia — complete heart block", "aliases": ["complete heart block"] });
    brady_v["meta"]["search_tags"] = serde_json::json!(["synthetic", "bradycardia", "test"]);
    brady_v["exam_findings"][0]["value"] = serde_json::json!("78/50 mmHg");
    brady_v["exam_findings"][1]["value"] = serde_json::json!("36/min, regular");
    brady_v["exam_findings"][2]["value"] = serde_json::json!("18/min");
    brady_v["exam_findings"][3]["value"] = serde_json::json!("95% on room air");
    brady_v["exam_findings"][4]["value"] = serde_json::json!("Pale, sweaty, GCS 15");
    brady_v["hidden"]["red_flags"] = serde_json::json!(["Hypotension with a rate of 36 = unstable bradycardia — atropine now, pads on"]);
    brady_v["hidden"]["management_plan"] = serde_json::json!(["Atropine 1 mg IV", "Transcutaneous pacing if atropine fails", "Admit to a monitored bed"]);
    let brady = pack_of(&brady_v.to_string());
    let svt = pack_of(&svt("aac-svt-third-by-name", "108/70 mmHg", &[]));
    // handed to the report in name order — PEA, brady, SVT, then the VF fixture — as `--all` would
    let results = vec![
        (pea.case_id.clone(), Outcome::Compiled(Box::new(pea))),
        (brady.case_id.clone(), Outcome::Compiled(Box::new(brady))),
        (svt.case_id.clone(), Outcome::Compiled(Box::new(svt))),
        (vf.case_id.clone(), Outcome::Compiled(Box::new(vf))),
    ];
    let report = render(&results, "lib", "test", None);
    let section = report.split("## ACLS scenarios").nth(1).expect("an ACLS section");
    let section = section.split("\n## ").next().unwrap();
    let pos = |needle: &str| section.find(needle).unwrap_or_else(|| panic!("{needle} not in the ACLS section:\n{section}"));
    assert!(pos("synthetic-vf-arrest-test") < pos("aaa-pea-first-by-name"), "VF first");
    assert!(pos("aaa-pea-first-by-name") < pos("aab-brady-second-by-name"), "PEA/asystole second");
    assert!(pos("aab-brady-second-by-name") < pos("aac-svt-third-by-name"), "bradycardia third, the tachycardias after");
    assert!(section.contains("VF/pVT") && section.contains("PEA/asystole") && section.contains("bradycardia"), "{section}");
}

// ── 3.5 ───────────────────────────────────────────────────────────────────────────────

#[test]
fn r3_5_the_kit_shock_button_earns_the_defibrillation_item_the_typed_order_earns() {
    let pack = pack_of(VF);
    let item = pack.rubric["items"].as_array().unwrap().iter()
        .find(|i| i["needle"] == "tx_defibrillate")
        .unwrap_or_else(|| panic!("no rubric item for the shock: {}", pack.rubric["items"]));
    assert_eq!(item["type"], "action_by", "the shock is timed, not only present");
    let label = item["label"].as_str().unwrap().to_string();
    let sce_json = pack.sce.to_string();
    let rubric_json = pack.rubric.to_string();
    let earned = |tape: &[Step]| -> bool {
        let (_, det) = vitals_osce::sheet_for_run(&sce_json, tape, &rubric_json).expect("mark sheet");
        det.items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("{label} missing from the sheet")).earned
    };
    let typed = vec![Step::Tick(10.0), Step::Act { text: "cpr".into(), id: "tx_cpr".into() }, Step::Act { text: "shock 200".into(), id: "tx_defibrillate".into() }, Step::Tick(1.0)];
    assert!(earned(&typed), "the typed order earns it");
    let button = vec![Step::Tick(10.0), Step::Act { text: "cpr".into(), id: "tx_cpr".into() }, Step::Shock(200.0), Step::Tick(1.0)];
    assert!(earned(&button), "the kit button earns it too — same shock, same VF, same second");
    let late = vec![Step::Tick(10.0), Step::Act { text: "cpr".into(), id: "tx_cpr".into() }, Step::Tick(200.0), Step::Shock(200.0), Step::Tick(1.0)];
    assert!(!earned(&late), "a shock after the window is not on time");
    // and into PEA the button earns nothing — the shock was not indicated
    let mut v: serde_json::Value = serde_json::from_str(VF).unwrap();
    v["meta"]["id"] = serde_json::json!("synthetic-pea-button-test");
    v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": "Cardiac arrest — pulseless electrical activity", "aliases": ["PEA arrest", "cardiac arrest"] });
    let pea = pack_of(&v.to_string());
    assert!(pea.rubric["items"].as_array().unwrap().iter().all(|i| i["needle"] != "tx_defibrillate"), "PEA prices no shock");
}

// ── 3.6 ───────────────────────────────────────────────────────────────────────────────

#[test]
fn r3_6_a_childs_shock_pays_for_the_antibiotics_and_the_glucose_beside_the_fluid_already() {
    let Some(root) = library() else { return common::skip("embla-cases-world not present") };
    for (id, want) in [
        ("embla-severe-acute-malnutrition-septic-shock-child-resident", &["tx_fluids", "tx_dextrose", "tx_antibiotics"][..]),
        ("embla-cholera-severe-dehydration-shock-child-student", &["tx_fluids", "tx_dextrose", "tx_antibiotics"][..]),
        ("embla-dengue-shock-syndrome-child-intern", &["tx_fluids", "tx_dextrose"][..]),
    ] {
        let pack = library_pack(&root, id).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pack.archetype, "paediatric_compensated_shock", "{id}");
        let paid: Vec<String> = pack.rubric["items"].as_array().unwrap().iter()
            .filter(|i| i["type"] == "action" || i["type"] == "action_by")
            .map(|i| i["needle"].as_str().unwrap().to_string()).collect();
        for w in want {
            assert!(paid.contains(&w.to_string()), "{id}: {w} not paid — {paid:?}");
        }
        let turn = pack.sce["states"][0]["transitions"][1]["when"].to_string();
        for w in want {
            assert!(turn.contains(w), "{id}: {w} not part of the turn — {turn}");
        }
    }
}

// ── 3.7 ───────────────────────────────────────────────────────────────────────────────

fn adult_with_vitals(id: &str, dx: &str, aliases: &[&str], vitals: &[&str], plan: &[&str], age: u32) -> String {
    let mut v: serde_json::Value = serde_json::from_str(SEPTIC).unwrap();
    v["meta"]["id"] = serde_json::json!(id);
    v["meta"]["title"] = serde_json::json!("test");
    v["patient"]["age"] = serde_json::json!(age);
    v["hidden"]["correct_diagnosis"] = serde_json::json!({ "display": dx, "aliases": aliases });
    v["meta"]["search_tags"] = serde_json::json!(["synthetic", "test"]);
    v["hidden"]["red_flags"] = serde_json::json!([]);
    v["hidden"]["management_plan"] = serde_json::json!(plan);
    for (i, val) in vitals.iter().enumerate() {
        v["exam_findings"][i]["value"] = serde_json::json!(val);
    }
    v.to_string()
}

#[test]
fn r3_7_an_adult_whose_presenting_vitals_score_news2_low_is_cut_by_name() {
    // a shock index of 1.04 at a systolic of 104 passes the haemorrhagic gate; NEWS2 says 2, nothing red
    let s = adult_with_vitals("synthetic-normal-vitals", "Upper gastrointestinal haemorrhage with haemorrhagic shock", &["GI bleeding"],
        &["104/70 mmHg", "108/min", "18/min", "97% on room air", "37.0 °C", "Pale; GCS 15"],
        &["Two large-bore lines, crystalloid 1 L", "Transfuse packed red cells", "Urgent endoscopy for haemostasis", "Pantoprazole 80 mg IV"], 45);
    let err = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_err();
    assert!(err.reason.contains("NEWS2"), "{}", err.reason);
    assert!(err.reason.contains("3.7"), "names the ruling: {}", err.reason);
    assert!(err.reason.contains("within normal") || err.reason.contains("low"), "{}", err.reason);
    assert_eq!(vitals_casefactory::report::reason_family(&err.reason), "presenting vitals within normal (NEWS2 low — cut by the advisor's rule 3.7)");
    // the same numbers on a child are not NEWS2's to read: the paediatric gate decides
    let child = adult_with_vitals("synthetic-child-vitals", "Dengue shock syndrome", &["dengue shock"],
        &["104/86 mmHg", "108/min", "18/min", "97% on room air", "37.0 °C", "GCS 15"],
        &["Isotonic crystalloid 10 mL/kg/h, reassessed hourly", "Admit"], 8);
    let r = compile(&child, Source::of("embla-cases", "test", &child));
    assert!(r.as_ref().err().is_none_or(|e| !e.reason.contains("NEWS2")), "{:?}", r.err().map(|e| e.reason));
    // and one red parameter keeps a low total on the ward
    let red = adult_with_vitals("synthetic-one-red", "Upper gastrointestinal haemorrhage with haemorrhagic shock", &["GI bleeding"],
        &["88/60 mmHg", "80/min", "18/min", "97% on room air", "37.0 °C", "Pale; GCS 15"],
        &["Two large-bore lines, crystalloid 1 L", "Transfuse packed red cells", "Urgent endoscopy for haemostasis"], 45);
    pack_of(&red);
}

#[test]
fn r3_7_the_library_cut_is_three_translated_cases_and_no_endemic_one() {
    let Some(root) = library() else { return common::skip("embla-cases-world not present") };
    for id in ["ddx-anemia-4-en", "embla-ectopic-pregnancy-1-en", "ddx-spontaneous-pneumothorax-1-en"] {
        let err = library_pack(&root, id).unwrap_err();
        assert!(err.contains("NEWS2"), "{err}");
    }
    for id in common::ENDEMIC_SIX {
        library_pack(&root, id).unwrap_or_else(|e| panic!("{e}"));
    }
    // and the report names the cut under its own heading
    let s = adult_with_vitals("synthetic-normal-vitals", "Upper gastrointestinal haemorrhage with haemorrhagic shock", &["GI bleeding"],
        &["104/70 mmHg", "108/min", "18/min", "97% on room air", "37.0 °C", "Pale; GCS 15"],
        &["Crystalloid 1 L", "Transfuse packed red cells", "Urgent endoscopy for haemostasis"], 45);
    let err = compile(&s, Source::of("embla-cases", "test", &s)).unwrap_err();
    let report = render(&[("synthetic-normal-vitals".to_string(), Outcome::Refused(err))], "lib", "test", None);
    assert!(report.contains("### presenting vitals within normal (NEWS2 low — cut by the advisor's rule 3.7) — 1"), "{report}");
}
