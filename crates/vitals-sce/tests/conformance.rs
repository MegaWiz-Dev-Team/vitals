//! Conformance: this implementation must reproduce the reference engine's behaviour exactly.
//!
//! The vectors in `conformance/ep1-vectors.json` were frozen once from Embla's `sce_runtime`,
//! the reference implementation. They are the contract between the two — which is why neither
//! crate depends on the other, and why a failing test here is never fixed by regenerating the
//! vectors. Regenerate only when the scenario or the specification changes on purpose.

use serde::Deserialize;
use std::path::PathBuf;
use vitals_sce::{render_beat, Sce, SceState};

#[derive(Deserialize)]
struct Vectors {
    sce_hash: String,
    vectors: Vec<Vector>,
}

#[derive(Deserialize)]
struct Vector {
    name: String,
    tape: Vec<Step>,
    beats: Vec<String>,
    harm_events: Vec<String>,
    outcome: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Step {
    Tick { tick: f64 },
    Do { r#do: String },
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn matches_reference_engine_on_every_vector() {
    let root = repo_root();
    let vectors: Vectors = serde_json::from_str(
        &std::fs::read_to_string(root.join("conformance/ep1-vectors.json"))
            .expect("conformance/ep1-vectors.json"),
    )
    .expect("vectors parse");

    let sce_json = std::fs::read_to_string(root.join("conformance/sce-anaphylaxis-ep1.json"))
        .expect("conformance/sce-anaphylaxis-ep1.json — the scenario the vectors were frozen against");

    assert!(!vectors.sce_hash.is_empty(), "vectors must name the scenario they came from");

    let mut failures = Vec::new();

    for v in &vectors.vectors {
        let sce = Sce::from_json(&sce_json).expect("scenario parses");
        let mut st = SceState::new(sce);
        let mut beats = Vec::new();

        for step in &v.tape {
            let emitted = match step {
                Step::Tick { tick } => st.tick(*tick),
                Step::Do { r#do } => st.apply(r#do),
            };
            beats.extend(emitted.iter().map(render_beat));
        }

        let outcome = st.outcome().map(|o| format!("{o:?}"));

        if beats != v.beats {
            failures.push(format!("[{}] beats\n  expected {:?}\n  got      {:?}", v.name, v.beats, beats));
        }
        if st.harm_events != v.harm_events {
            failures.push(format!("[{}] harm\n  expected {:?}\n  got      {:?}", v.name, v.harm_events, st.harm_events));
        }
        if outcome != v.outcome {
            failures.push(format!("[{}] outcome\n  expected {:?}\n  got      {:?}", v.name, v.outcome, outcome));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} vectors diverge from the reference engine:\n\n{}\n\nDo not regenerate the vectors to make this pass.",
        failures.len(),
        vectors.vectors.len(),
        failures.join("\n\n")
    );
}

#[test]
fn replaying_the_same_tape_twice_is_identical() {
    let root = repo_root();
    let sce_json = std::fs::read_to_string(root.join("conformance/sce-anaphylaxis-ep1.json")).unwrap();
    let run = || {
        let mut st = SceState::new(Sce::from_json(&sce_json).unwrap());
        let mut beats = Vec::new();
        for s in ["adrenaline im", "oxygen", "supine"] {
            beats.extend(st.tick(30.0).iter().map(render_beat));
            beats.extend(st.apply(s).iter().map(render_beat));
        }
        beats.extend(st.tick(900.0).iter().map(render_beat));
        (beats, st.harm_events.clone(), st.outcome().map(|o| format!("{o:?}")))
    };
    assert_eq!(run(), run(), "the interpreter must be a pure function of the tape");
}
