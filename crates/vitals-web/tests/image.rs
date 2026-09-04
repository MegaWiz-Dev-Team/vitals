//! Everything the server reads at runtime has to be in the image.
//!
//! `demo/personas` was not, from the day the stations got voices until this test existed. Nothing
//! failed: the server reads a persona per case off the disk and a case with no file plays mute
//! rather than erroring, which is right for a case that has no persona and wrong for a case whose
//! persona simply never arrived. Production printed `personas 1/17 voiced` at every boot and
//! served twelve silent patients.
//!
//! So this is not a test that `demo/personas` is copied. It is a test that **every directory the
//! server loads from is copied**, because the next one added will be forgotten the same way.

use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The runtime roots, named here because the code that reads them names them separately:
/// `scenario_path`, `persona_path`, `rubric_path` and the archive. A path that appears in any of
/// those and not in the Dockerfile is a file the server will look for and not find.
const RUNTIME_PATHS: &[&str] = &[
    "demo/scenarios",
    "demo/stations",
    "demo/rubrics",
    "demo/personas",
    "demo/ep1-en.json",
    "conformance",
];

#[test]
fn the_image_carries_every_path_the_server_reads() {
    let root = repo();
    let dockerfile = std::fs::read_to_string(root.join("Dockerfile")).expect("Dockerfile");
    let copied: Vec<&str> = dockerfile
        .lines()
        .filter_map(|l| l.trim().strip_prefix("COPY "))
        .flat_map(|l| l.split_whitespace().next())
        .collect();

    for path in RUNTIME_PATHS {
        assert!(
            root.join(path).exists(),
            "{path} is listed as a runtime path and is not in the repository"
        );
        assert!(
            copied.contains(path),
            "the server reads {path} at runtime and the Dockerfile does not copy it — in the \
             image it will simply not be there, and the failure will be silence rather than an \
             error. Copied: {copied:?}"
        );
    }
}

/// Every case that has a persona file must be one the server can actually play.
///
/// A persona for a case id nothing serves is a file nobody reads; the inverse — a case with no
/// persona — is allowed and visible, and `/api/chain`'s `voiced` reports it.
#[test]
fn every_persona_belongs_to_a_case() {
    let root = repo();
    let personas = std::fs::read_dir(root.join("demo/personas")).expect("demo/personas");
    for e in personas.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().replace(".json", "");
        let station = root.join(format!("demo/stations/{name}.sce.json"));
        let scenario = root.join(format!("demo/scenarios/{name}.json"));
        assert!(
            station.exists() || scenario.exists(),
            "demo/personas/{name}.json has no case behind it — nothing will ever read it"
        );
    }
}

/// How many cases can speak, so the number in verify-deploy.sh comes from the tree.
///
/// Today it is thirteen: twelve stations with persona files, plus ep1, which reads its own
/// scenario. **ep2 to ep5 have no persona and are mute**, which is a real gap and not one the
/// image can fix — the files do not exist. Naming it here stops thirteen being read as "all".
#[test]
fn the_voiced_count_is_the_personas_plus_ep1() {
    let root = repo();
    let personas: Vec<String> = std::fs::read_dir(root.join("demo/personas"))
        .expect("demo/personas")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().replace(".json", ""))
        .collect();
    assert_eq!(personas.len(), 12, "the persona count moved: {personas:?}");
    for mute in ["ep2", "ep3", "ep4", "ep5"] {
        assert!(
            !personas.contains(&mute.to_string()),
            "{mute} has a persona now — the voiced count in scripts/verify-deploy.sh is derived \
             from this directory, so it follows automatically, but the four numbered episodes \
             being mute was a known gap and this test is where it was written down"
        );
    }
}
