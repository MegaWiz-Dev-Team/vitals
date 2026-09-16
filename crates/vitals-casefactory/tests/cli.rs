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
    assert!(report.contains("refused 1"), "{report}");
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
