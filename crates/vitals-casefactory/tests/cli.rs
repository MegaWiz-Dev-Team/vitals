//! The binary: one case to one pack file, or the whole library to packs plus a report — and a
//! refused case leaves no file behind.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

const SYNTHETIC: &str = include_str!("fixtures/synthetic-septic-shock.json");

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vitals-casefactory-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A tiny library on disk: the synthetic case, and a copy of it made stable (refused).
fn library(root: &Path) -> PathBuf {
    let lib = root.join("embla-cases");
    let a = lib.join("cases").join("synthetic-septic-shock-test");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::write(a.join("case.json"), SYNTHETIC).unwrap();
    let mut stable: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    stable["meta"]["id"] = serde_json::json!("synthetic-stable-test");
    stable["exam_findings"][0]["value"] = serde_json::json!("124/80 mmHg");
    let b = lib.join("cases").join("synthetic-stable-test");
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(b.join("case.json"), stable.to_string()).unwrap();
    // a third copy, compilable, but recorded as already deployed to the season
    let mut season: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    season["meta"]["id"] = serde_json::json!("synthetic-season-source-test");
    let c = lib.join("cases").join("synthetic-season-source-test");
    std::fs::create_dir_all(&c).unwrap();
    std::fs::write(c.join("case.json"), season.to_string()).unwrap();
    std::fs::write(
        lib.join("deployments.jsonl"),
        concat!(
            "{\"case_id\": \"synthetic-season-source-test\", \"target\": \"vitals\", \"deployed_version\": \"1.0.0\", \"status\": \"active\"}\n",
            "{\"case_id\": \"synthetic-septic-shock-test\", \"target\": \"cloud\", \"deployed_version\": \"1.0.0\", \"status\": \"active\"}\n",
        ),
    )
    .unwrap();
    lib
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_vitals-casefactory")).args(args).output().expect("the binary runs");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn compile_one_case_writes_one_pack_file() {
    let root = scratch("one");
    let lib = library(&root);
    let out = root.join("out");
    let (ok, stdout, stderr) = run(&["compile", "--cases", lib.to_str().unwrap(), "--id", "synthetic-septic-shock-test", "--out", out.to_str().unwrap()]);
    assert!(ok, "stdout: {stdout}\nstderr: {stderr}");
    let pack_path = out.join("synthetic-septic-shock-test.pack.json");
    let pack: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&pack_path).unwrap()).unwrap();
    assert_eq!(pack["case_id"], "synthetic-septic-shock-test");
    assert_eq!(pack["archetype"], "septic_shock");
    assert_eq!(pack["source"]["repo"], "embla-cases");
    assert_eq!(pack["source"]["ref"], "worktree");
    assert!(stdout.contains("compiled"), "{stdout}");
}

#[test]
fn a_refused_case_writes_nothing_and_exits_nonzero() {
    let root = scratch("refused");
    let lib = library(&root);
    let out = root.join("out");
    let (ok, stdout, stderr) = run(&["compile", "--cases", lib.to_str().unwrap(), "--id", "synthetic-stable-test", "--out", out.to_str().unwrap()]);
    assert!(!ok);
    assert!(!out.join("synthetic-stable-test.pack.json").exists());
    assert!(stderr.contains("refused") || stdout.contains("refused"), "stdout: {stdout}\nstderr: {stderr}");
}

#[test]
fn all_compiles_the_library_and_writes_a_report() {
    let root = scratch("all");
    let lib = library(&root);
    let out = root.join("out");
    let (ok, stdout, stderr) = run(&["compile", "--cases", lib.to_str().unwrap(), "--all", "--out", out.to_str().unwrap()]);
    assert!(ok, "stdout: {stdout}\nstderr: {stderr}");
    assert!(out.join("synthetic-septic-shock-test.pack.json").exists());
    assert!(!out.join("synthetic-stable-test.pack.json").exists());
    let report = std::fs::read_to_string(out.join("REPORT.md")).unwrap();
    assert!(report.contains("compiled 1"), "{report}");
    assert!(report.contains("refused 2"), "{report}");
    // the season's source is refused by name and listed on its own
    assert!(!out.join("synthetic-season-source-test.pack.json").exists());
    assert!(report.contains("season source"), "{report}");
    assert!(report.contains("## Season sources"), "{report}");
    assert!(report.contains("synthetic-season-source-test"), "{report}");
    assert!(report.contains("septic_shock"), "{report}");
    assert!(report.contains("synthetic-stable-test"), "{report}");
    assert!(report.contains("not forced"), "{report}");
}

#[test]
fn a_git_ref_after_an_at_sign_reads_the_case_from_that_ref() {
    let Some(dir) = common::embla_dir() else { return common::skip("embla-cases not present"); };
    if common::show(&dir, common::ENDEMIC_REF, common::ENDEMIC_SIX[0]).is_none() {
        return common::skip("endemic branch not present");
    }
    let root = scratch("ref");
    let out = root.join("out");
    let spec = format!("{}@{}", dir.display(), common::ENDEMIC_REF);
    let (ok, stdout, stderr) = run(&["compile", "--cases", &spec, "--id", common::ENDEMIC_SIX[0], "--out", out.to_str().unwrap()]);
    assert!(ok, "stdout: {stdout}\nstderr: {stderr}");
    let pack: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(out.join(format!("{}.pack.json", common::ENDEMIC_SIX[0]))).unwrap()).unwrap();
    assert_eq!(pack["source"]["ref"], common::ENDEMIC_REF);
    assert_eq!(pack["endemic"], true);
}

#[test]
fn a_season_source_is_refused_even_when_asked_for_by_id() {
    let root = scratch("season");
    let lib = library(&root);
    let out = root.join("out");
    let (ok, stdout, stderr) = run(&["compile", "--cases", lib.to_str().unwrap(), "--id", "synthetic-season-source-test", "--out", out.to_str().unwrap()]);
    assert!(!ok);
    assert!(!out.join("synthetic-season-source-test.pack.json").exists());
    assert!((stdout + &stderr).contains("season source"));
}

#[test]
fn the_binary_reads_the_review_block_beside_the_case_and_clears_the_flag_only_when_reviewed() {
    let root = scratch("review");
    let lib = library(&root);
    std::fs::write(
        lib.join("cases").join("synthetic-septic-shock-test").join("case.meta.yaml"),
        "id: synthetic-septic-shock-test\nversion: 0.0.1\nworld:\n  world_ready: true\n  country: ZZZ\n  endemic: false\n  review:\n    status: reviewed\n    by: clinical-advisor\n    date: 2026-09-20\n  compiled: null\ndeployments: []\n",
    )
    .unwrap();
    let out = root.join("out");
    let (ok, stdout, stderr) = run(&["compile", "--cases", lib.to_str().unwrap(), "--id", "synthetic-septic-shock-test", "--out", out.to_str().unwrap()]);
    assert!(ok, "stdout: {stdout}\nstderr: {stderr}");
    let pack: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(out.join("synthetic-septic-shock-test.pack.json")).unwrap()).unwrap();
    assert_eq!(pack["provisional"], false);
    assert_eq!(pack["review"]["by"], "clinical-advisor");
    assert!(pack["rubric"]["status"].as_str().unwrap().contains("reviewed by the clinical advisor on 2026-09-20"));
    // a copy with no meta file at all stays provisional
    let mut copy: serde_json::Value = serde_json::from_str(SYNTHETIC).unwrap();
    copy["meta"]["id"] = serde_json::json!("synthetic-no-meta-test");
    let b = lib.join("cases").join("synthetic-no-meta-test");
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(b.join("case.json"), copy.to_string()).unwrap();
    let (ok, stdout, stderr) = run(&["compile", "--cases", lib.to_str().unwrap(), "--id", "synthetic-no-meta-test", "--out", out.to_str().unwrap()]);
    assert!(ok, "stdout: {stdout}\nstderr: {stderr}");
    let pack: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(out.join("synthetic-no-meta-test.pack.json")).unwrap()).unwrap();
    assert_eq!(pack["provisional"], true);
    assert!(pack["review"].is_null());
}
