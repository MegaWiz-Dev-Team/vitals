//! The diastolic moved, and nothing a run is judged on may move with it.
//!
//! `vitals-sce` derives the diastolic from the systolic for any case that does not drive `dbp`
//! itself. Before that, a case declaring dynamics for `sbp` alone left the diastolic standing
//! exactly where `vitals0` put it while the systolic marched down through it — `osce-a` printed
//! `62/60`, then `58/58`, which is not a low blood pressure but a reading with no pulse pressure
//! at all.
//!
//! Changing a vital sign inside an engine whose runs are anchored on a chain is only safe if the
//! change cannot reach a leaf. Two facts make it safe here, and this file checks both rather
//! than asserting them:
//!
//!   * **Nothing reads `dbp`.** In all seventeen cases the only occurrence of `dbp` is the
//!     `vitals0` line — no dynamic, no band, no transition, no trigger, no effect. So no
//!     condition can evaluate differently and no beat, harm or ending can move.
//!   * **The leaf never held it anyway.** `vitals_replay::leaf` commits to the tape, the beats,
//!     the harms and the outcome, never to a trajectory.
//!
//! The hashes and scores below were recorded from the build immediately *before* the derivation
//! landed. A value that changes here is a run somebody may already have anchored.
//!
//! `cargo test -p vitals-osce --test diastolic_neutrality -- --nocapture` prints the table.

use std::path::PathBuf;
use vitals_osce::sheet_for_run;
use vitals_replay::{leaf, replay, sce_hash, Step};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// One scripted run, played identically against every case: some time, two orders, a question,
/// and then long enough for any of these scenarios to reach its ending. Blunt on purpose — this
/// is a fixture for hashes, not a demonstration of good medicine.
fn tape() -> Vec<Step> {
    let mut t = vec![
        Step::Tick(30.0),
        Step::did("adrenaline im"),
        Step::Tick(30.0),
        Step::asked("what happened?"),
        Step::did("give oxygen"),
    ];
    for _ in 0..30 {
        t.push(Step::Tick(60.0));
    }
    t
}

/// `(case, rubric, outcome, det score, leaf)` — as of the build before the diastolic moved.
/// An empty rubric name means the case ships none (the conformance copy of EP1); that row
/// checks the leaf and the outcome only.
struct Pin {
    case: &'static str,
    rubric: &'static str,
    outcome: &'static str,
    score: &'static str,
    leaf: &'static str,
}

const PINNED: &[Pin] = &[
    // ── the twelve stations, re-issued 2026-09: every silent order got its beat ─────────────
    // Same tape, same outcome, same det score on every row — the marking never reads a beat —
    // and every leaf moved, twice over: a leaf commits to `sce_hash`, which any edit rotates,
    // and to the beats themselves, which this re-issue added to. The versions the pins below
    // were recorded against are archived under `conformance/sce-archive/` and
    // `crates/vitals-replay/tests/shock_tape.rs` still holds their leaves where they were,
    // which is the check that matters for a run somebody has already anchored.
    Pin { case: "demo/stations/osce-a.sce.json", rubric: "demo/rubrics/osce-a.json", outcome: "WinDischarge", score: "24/40", leaf: "70d50e25d5e737e15208145480d1ce274d4d9623e7ebd2537a98c92ba569cf8b" },
    Pin { case: "demo/stations/osce-a2.sce.json", rubric: "demo/rubrics/osce-a2.json", outcome: "WinDischarge", score: "23/40", leaf: "1076b048bef20369cff4d5c2c5e36001534c2d781e33c3d521a6667e8f3d1c44" },
    Pin { case: "demo/stations/osce-b.sce.json", rubric: "demo/rubrics/osce-b.json", outcome: "DeathArrest", score: "0/40", leaf: "1c823ea43de5869779d6a2f489e297da6551bea03731c483b41ee0097842763c" },
    Pin { case: "demo/stations/osce-b2.sce.json", rubric: "demo/rubrics/osce-b2.json", outcome: "-", score: "8/40", leaf: "c9a49eb7bdd384c86d5d5f0d92ab1c5e234eb86a4b3c6ac4748a900c59f9bf78" },
    Pin { case: "demo/stations/osce-b3.sce.json", rubric: "demo/rubrics/osce-b3.json", outcome: "DeathArrest", score: "2/40", leaf: "ccac120b006a9adec0d6863ec5a02c8b1998dbb7eb7d39c6162783342d688c79" },
    Pin { case: "demo/stations/osce-c.sce.json", rubric: "demo/rubrics/osce-c.json", outcome: "-", score: "16/40", leaf: "998d264cfd1c666c51bb81a2052fa9ee642c2f6a4fc4660efa3bd2ae4439a3e5" },
    Pin { case: "demo/stations/osce-c2.sce.json", rubric: "demo/rubrics/osce-c2.json", outcome: "DeathArrest", score: "8/40", leaf: "899f654b0a78455b93d08f9f51f1add220dfc29bb5eaed337fc254a8ea50bb4f" },
    Pin { case: "demo/stations/osce-c3.sce.json", rubric: "demo/rubrics/osce-c3.json", outcome: "DeathArrest", score: "6/40", leaf: "5ccc2bd82618af448e25a2858313cf70ab857aab99d5bd3895cbe8262644156b" },
    Pin { case: "demo/stations/osce-d.sce.json", rubric: "demo/rubrics/osce-d.json", outcome: "DeathArrest", score: "2/40", leaf: "f2ab026e1c66a4ac8463f6b25b12a38228d52fd6aa1697d1ea487633cd4f8ad4" },
    Pin { case: "demo/stations/osce-d2.sce.json", rubric: "demo/rubrics/osce-d2.json", outcome: "DeathArrest", score: "8/40", leaf: "ac0bdff7fb18a24ac6e94b988b751fd6718694dcd54eca44aa78c77d1f4ef6c5" },
    Pin { case: "demo/stations/osce-d3.sce.json", rubric: "demo/rubrics/osce-d3.json", outcome: "DeathArrest", score: "10/40", leaf: "821e797f8c8d1a2a4f543237e7f29dd158724b492b397d8dd39d0dbe24350493" },
    Pin { case: "demo/stations/osce-d4.sce.json", rubric: "demo/rubrics/osce-d4.json", outcome: "DeathArrest", score: "5/40", leaf: "8ce2eae4837df72ea2043bb7b51de7220c06a15c0886b21c1e1d14159b74131d" },
    // ── re-issued 2026-08-29, when the defibrillator moved into the engine ──────────────────
    // The leaf moved and nothing about the *run* did: same tape, same beats, same harms, same
    // ending, same 10/40. A leaf commits to `sce_hash`, `ep2`'s file was re-issued, and that is
    // the whole of the difference — was
    // `f142bc45bb93bc336917fd81092601c66a727668eb23cb6552405781939eda9b`, against
    // `e39de235…1426d`, which is archived under `conformance/sce-archive/` and stays there.
    // `crates/vitals-replay/tests/shock_tape.rs` replays this same tape against every archived
    // version and holds each of those leaves where it was, which is the check that matters for a
    // run somebody has already anchored: an anchored run names the bytes it was played against,
    // and those bytes did not move.
    Pin { case: "demo/scenarios/ep2-stemi.json", rubric: "demo/rubrics/ep2-stemi.json", outcome: "DeathArrest", score: "10/40", leaf: "aa59eb3a797490a330b84bbc93de0738a42331829a7c2baaef00bbbb7bec3c29" },
    Pin { case: "demo/scenarios/ep3-epiglottitis.json", rubric: "demo/rubrics/ep3-epiglottitis.json", outcome: "DeathArrest", score: "14/40", leaf: "76e2338f3d42345feed1f8654a0e0e8641a55d5e66d088ea1dfe8396bee52327" },
    Pin { case: "demo/scenarios/ep4-pulmonary-embolism.json", rubric: "demo/rubrics/ep4-pulmonary-embolism.json", outcome: "DeathArrest", score: "13/40", leaf: "691bddb158c16a02b7a6c81904f28caba1cfe88dce95250490aed1e17def5e2b" },
    Pin { case: "demo/scenarios/ep5-the-night-the-stars-fell.json", rubric: "demo/rubrics/ep5-the-night-the-stars-fell.json", outcome: "DeathArrest", score: "9/40", leaf: "4f3d0873a520a389cb029cd64f42319a0be5f596328da385089d4bb685ae2a3c" },
    Pin { case: "conformance/sce-anaphylaxis-ep1.json", rubric: "", outcome: "WinDischarge", score: "-", leaf: "37a15be328e050407baefacb2de1367dd35777d7e2153f98b915419c3fa239cb" },
];

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn the_same_scripted_run_still_ends_the_same_way_scores_the_same_and_hashes_the_same() {
    let tape = tape();
    let mut wrong = Vec::new();
    for p in PINNED {
        let sce_json = read(p.case);
        let r = replay(&sce_json, &tape).unwrap_or_else(|e| panic!("{}: {e}", p.case));
        let outcome = r.outcome.clone().unwrap_or_else(|| "-".into());
        let got_leaf = hex(&leaf(&sce_hash(&sce_json), &tape, &r));
        let score = if p.rubric.is_empty() {
            "-".to_string()
        } else {
            let (_, det) = sheet_for_run(&sce_json, &tape, &read(p.rubric))
                .unwrap_or_else(|e| panic!("{}: {e}", p.case));
            format!("{}/{}", det.earned, det.max)
        };
        println!(
            "    Pin {{ case: \"{}\", rubric: \"{}\", outcome: \"{outcome}\", score: \"{score}\", leaf: \"{got_leaf}\" }},",
            p.case, p.rubric
        );
        if outcome != p.outcome || score != p.score || got_leaf != p.leaf {
            wrong.push(format!(
                "  {}\n    was {} {} {}\n    now {outcome} {score} {got_leaf}",
                p.case, p.outcome, p.score, p.leaf
            ));
        }
    }
    assert!(!PINNED.is_empty(), "the fixture is empty");
    assert!(wrong.is_empty(), "{} case(s) moved:\n{}", wrong.len(), wrong.join("\n"));
}
