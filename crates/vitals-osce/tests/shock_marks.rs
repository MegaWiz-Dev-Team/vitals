//! What EP2's mark sheet still pays for, now that the defibrillator is not in EP2's file.
//!
//! `ep2-stemi.json` used to declare a `defibrillate` intervention whose harm arms both spelled
//! the words *"unsynchronised shock"*, and its own `_harm_note` said why: the rubric's
//! `no_harm` needle is that substring, and a re-issue must not change what the item pays for.
//! The intervention is gone — shockability is physiology, and `SceState::defibrillate` now
//! answers for every case on the shelf — so the sentence moved from the scenario file into the
//! engine.
//!
//! Five marks out of forty ride on that move, and this is what holds them: *Avoided an
//! unsynchronised shock outside VF*, earned when the shock was right and lost when it was not,
//! from the same rubric file, unedited.
//!
//! It is also the answer to "what would a rubric item for this look like" on the other three
//! episodes, which have no such item — see the note at the foot of this file.

use vitals_osce::sheet_for_run;
use vitals_replay::Step;

fn read(rel: &str) -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

const ITEM: &str = "Avoided an unsynchronised shock outside VF";

/// EP2's clock: primary VF at four minutes, ninety seconds of it, then asystole and death.
/// `at` is where the shock goes.
fn ep2_tape(at: Option<Step>) -> Vec<Step> {
    let mut t = vec![Step::Tick(240.0)];
    if let Some(s) = at.clone() {
        // Inside the VF window: the shock a defibrillator is for.
        t.push(s);
    }
    t
}

fn marks(tape: &[Step]) -> Vec<(String, bool)> {
    let (_, det) = sheet_for_run(
        &read("demo/scenarios/ep2-stemi.json"),
        tape,
        &read("demo/rubrics/ep2-stemi.json"),
    )
    .expect("mark sheet");
    det.items.iter().map(|i| (i.label.clone(), i.earned)).collect()
}

fn earned(tape: &[Step]) -> bool {
    marks(tape)
        .into_iter()
        .find(|(l, _)| l == ITEM)
        .unwrap_or_else(|| panic!("the rubric no longer has the item {ITEM:?}"))
        .1
}

/// Shocking VF is the right answer, and the sheet says so by not taking anything.
#[test]
fn the_shock_the_case_is_about_costs_nothing() {
    assert!(earned(&ep2_tape(Some(Step::Shock(200.0)))), "a correct shock lost the avoidance mark");
}

/// Not shocking at all also earns it — it is an avoidance item, and it always was.
#[test]
fn never_touching_the_defibrillator_earns_it_too() {
    assert!(earned(&ep2_tape(None)));
}

/// **The five marks.** Ninety seconds of unshocked VF degenerate to asystole; a shock delivered
/// there is the error the item is named for, and it is charged exactly as it was when the
/// sentence lived in the scenario file.
#[test]
fn shocking_the_asystole_the_case_degenerates_into_costs_the_five_marks() {
    let tape = vec![
        Step::Tick(240.0), // primary VF
        Step::Tick(90.0),  // …unshocked, and it degenerates
        Step::Shock(200.0),
        Step::Tick(30.0),
    ];
    let sheet = marks(&tape);
    assert!(
        !sheet.iter().any(|(l, e)| l == ITEM && *e),
        "shocking asystole kept the avoidance mark — the needle no longer reaches the engine's \
         wording, and the item has become impossible to lose: {sheet:?}"
    );
    // Nothing else on the sheet moved. The item is a deduction of five, not a rewrite.
    let control = marks(&[Step::Tick(240.0), Step::Tick(90.0), Step::Tick(30.0)]);
    let differ: Vec<&str> = sheet
        .iter()
        .zip(control.iter())
        .filter(|((_, a), (_, b))| a != b)
        .map(|((l, _), _)| l.as_str())
        .collect();
    assert_eq!(differ, vec![ITEM], "the shock moved items it has nothing to do with: {differ:?}");
}

/// The same wrong shock on a patient who still has a pulse. The other harm sentence, and the
/// same needle — because *unsynchronised* is what makes both of them wrong.
#[test]
fn shocking_a_perfusing_stemi_costs_the_same_five_marks() {
    assert!(!earned(&[Step::Tick(60.0), Step::Shock(200.0), Step::Tick(30.0)]));
}

// ── what the other three sheets cannot do ───────────────────────────────────
//
// EP3, EP4 and EP5 arrest in PEA and have no avoidance item for a shock at all, so the run above
// — button pressed, harm recorded, chart line written — scores identically to a run in which
// nobody touched the defibrillator. That is deliberate and it is not this file's to change:
// every one of those sheets sums to exactly 40, so an item is not an addition, it is a
// re-weighting of the rest, and the weights are a clinical judgement.
//
// This test exists so that the day the reviewer rules, the shape of the answer is already
// proven: it is a `no_harm` on the substring the engine writes, and it behaves exactly as EP2's
// does above.
#[test]
fn the_other_three_sheets_are_silent_about_a_shock_and_this_test_says_so_out_loud() {
    for (case, rubric) in [
        ("demo/scenarios/ep3-epiglottitis.json", "demo/rubrics/ep3-epiglottitis.json"),
        ("demo/scenarios/ep4-pulmonary-embolism.json", "demo/rubrics/ep4-pulmonary-embolism.json"),
        (
            "demo/scenarios/ep5-the-night-the-stars-fell.json",
            "demo/rubrics/ep5-the-night-the-stars-fell.json",
        ),
    ] {
        let sce = read(case);
        let rb = read(rubric);
        let quiet = vec![Step::Tick(600.0), Step::Tick(600.0)];
        let shocked = vec![Step::Tick(600.0), Step::Shock(200.0), Step::Tick(600.0)];
        let (_, a) = sheet_for_run(&sce, &quiet, &rb).expect("sheet");
        let (_, b) = sheet_for_run(&sce, &shocked, &rb).expect("sheet");
        assert_eq!(
            (a.earned, a.max),
            (b.earned, b.max),
            "{case}: the sheet moved — this test is stale and the reviewer's item has landed"
        );
        assert_eq!(a.max, 40, "{case}: the sheet no longer sums to forty");
    }
}
