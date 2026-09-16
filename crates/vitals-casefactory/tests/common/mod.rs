//! Shared test plumbing: where the embla-cases library is, and how to read one of its cases.
//!
//! The library is proprietary and lives outside this repo, so no fixture here copies a case
//! out of it. Tests that need a real case look for the sibling checkout (or `EMBLA_CASES`) and
//! **skip with a printed line** when it is absent — CI has no library and must stay green
//! without pretending it checked anything.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

pub const ENDEMIC_REF: &str = "endemic/world-2026-09";

pub const ENDEMIC_SIX: [&str; 6] = [
    "embla-severe-falciparum-malaria-cerebral-resident",
    "embla-lassa-fever-haemorrhagic-shock-resident",
    "embla-acute-chagas-myocarditis-cardiogenic-shock-resident",
    "embla-typhoid-ileal-perforation-septic-shock-intern",
    "embla-neurotoxic-krait-envenoming-respiratory-failure-intern",
    "embla-dengue-shock-syndrome-child-intern",
];

pub fn embla_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("EMBLA_CASES") {
        let p = PathBuf::from(p);
        return p.join("cases").is_dir().then_some(p);
    }
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let p = here.join("../../../embla-cases");
    p.join("cases").is_dir().then_some(p)
}

/// `git show <ref>:cases/<id>/case.json` — read-only, never a checkout.
pub fn show(dir: &std::path::Path, git_ref: &str, id: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C").arg(dir)
        .arg("show").arg(format!("{git_ref}:cases/{id}/case.json"))
        .output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn library_case(dir: &std::path::Path, id: &str) -> Option<String> {
    std::fs::read_to_string(dir.join("cases").join(id).join("case.json")).ok()
}

pub fn skip(why: &str) {
    eprintln!("skip — {why}");
}
