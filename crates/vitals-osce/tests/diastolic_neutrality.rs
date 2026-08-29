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
    Pin { case: "demo/stations/osce-a.sce.json", rubric: "demo/rubrics/osce-a.json", outcome: "WinDischarge", score: "24/40", leaf: "312b3b4c82577d1341c2ccc46fac4e1cd171ef1f0fd634d4538c91871c0c76c3" },
    Pin { case: "demo/stations/osce-a2.sce.json", rubric: "demo/rubrics/osce-a2.json", outcome: "WinDischarge", score: "23/40", leaf: "4516ad22cefeda89fecdb497fb6c510b9771246bd4c667ad7a0faf95e1c8bdf8" },
    Pin { case: "demo/stations/osce-b.sce.json", rubric: "demo/rubrics/osce-b.json", outcome: "DeathArrest", score: "0/40", leaf: "abe44c768b5d2b07025fae3e278fa66845830c2ea7b35b62340d9f4c4bc9686d" },
    Pin { case: "demo/stations/osce-b2.sce.json", rubric: "demo/rubrics/osce-b2.json", outcome: "-", score: "8/40", leaf: "50e67a1b1601d1e649c4d7f4511762f1da6de03691204bbd4a3d43f9cb8ca1a5" },
    Pin { case: "demo/stations/osce-b3.sce.json", rubric: "demo/rubrics/osce-b3.json", outcome: "DeathArrest", score: "2/40", leaf: "ec214156a67899f33debf1fe4d8b7211660144aecedcd651fab34798b91f9fea" },
    Pin { case: "demo/stations/osce-c.sce.json", rubric: "demo/rubrics/osce-c.json", outcome: "-", score: "16/40", leaf: "d3255a811799ce6f6063dac62b4f5878cb7b09e0f49922821163d33789e92ac0" },
    Pin { case: "demo/stations/osce-c2.sce.json", rubric: "demo/rubrics/osce-c2.json", outcome: "DeathArrest", score: "8/40", leaf: "c74e032edc58b1869261ed4aee6e15b2a5441f913ad1a0c1a9bf4d75de6ba2e2" },
    Pin { case: "demo/stations/osce-c3.sce.json", rubric: "demo/rubrics/osce-c3.json", outcome: "DeathArrest", score: "6/40", leaf: "61f5e4b1d1a0542cd5cc1f4ed2732413ebea295d3c2643d613b0a5e90f902f0d" },
    Pin { case: "demo/stations/osce-d.sce.json", rubric: "demo/rubrics/osce-d.json", outcome: "DeathArrest", score: "2/40", leaf: "32cf55975c4c053469f7d13d2ab44b40c166a20dc1df8e31534411105796c780" },
    Pin { case: "demo/stations/osce-d2.sce.json", rubric: "demo/rubrics/osce-d2.json", outcome: "DeathArrest", score: "8/40", leaf: "94df2fc1f10d7390928e021485ea459372e5ac604b168c7750862e61076ccdc0" },
    Pin { case: "demo/stations/osce-d3.sce.json", rubric: "demo/rubrics/osce-d3.json", outcome: "DeathArrest", score: "10/40", leaf: "27aa65997a6d479a057dcb68b941a0523ca2ea5682618d867abe15379fa5f9f3" },
    Pin { case: "demo/stations/osce-d4.sce.json", rubric: "demo/rubrics/osce-d4.json", outcome: "DeathArrest", score: "5/40", leaf: "a95f6ce76f4f33f296da1ea65f6c2779b6f8abd5afed4cd09bd5cdd87bdd7fe6" },
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
