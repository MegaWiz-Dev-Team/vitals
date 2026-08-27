//! OSCE exam-mode demo — the star, made visible.
//!
//! Replays two runs of the same STEMI station, scores each against the deterministic rubric
//! (`vitals-osce`), and shows which earns a star. The score reads only what the automaton *did* —
//! `SceState::events()` after a byte-for-byte replay — so a stranger re-running this gets the same
//! number. That re-derivability is the whole reason a star may ride on chain.
//!
//! Run from the repo root:  `cargo run -p vitals-cli --bin osce`
use vitals_osce::{score, Rubric};
use vitals_replay::{resume, Step};

fn do_(s: &str) -> Step {
    Step::Do(s.into())
}

/// A competent pass: ECG, aspirin, oxygen, then reperfusion, then time to salvage muscle.
fn tape_competent() -> Vec<Step> {
    vec![
        do_("12-lead ecg"),
        Step::Tick(20.0),
        do_("aspirin 300 chewed"),
        Step::Tick(15.0),
        do_("oxygen"),
        Step::Tick(15.0),
        do_("activate the cath lab for pci"),
        Step::Tick(200.0),
        Step::Tick(60.0),
    ]
}

/// A shaky run: chase the troponin (the trap), reach for a nitrate, never reperfuse.
fn tape_shaky() -> Vec<Step> {
    vec![
        do_("wait for the troponin"),
        Step::Tick(60.0),
        do_("give a nitrate"),
        Step::Tick(60.0),
        Step::Tick(180.0),
    ]
}

fn main() {
    let sce = std::fs::read_to_string("demo/scenarios/ep2-stemi.json")
        .expect("run from repo root: demo/scenarios/ep2-stemi.json");
    let rubric: Rubric = serde_json::from_str(
        &std::fs::read_to_string("demo/rubrics/ep2-stemi.json").expect("demo/rubrics/ep2-stemi.json"),
    )
    .expect("rubric json");

    println!("station: {}  ·  pass {}%  ·  {}\n", rubric.case, rubric.pass_bps / 100, rubric.status);

    for (name, tape) in [("competent", tape_competent()), ("shaky", tape_shaky())] {
        let (st, replay) = resume(&sce, &tape).expect("replay");
        // The terminal goes in with the events: a run that ended with the patient dead is
        // capped under the pass bar whatever else it did (`vitals_osce::death_cap`).
        let det = score(st.events(), &rubric, st.outcome());

        println!("── {name} run ──────────────────────────────");
        print!("  events:");
        for e in st.events() {
            print!(" [{}·{:.0}s {}]", e.kind, e.t_sec, e.text);
        }
        println!("\n  outcome: {:?}   harms: {:?}", replay.outcome, replay.harm_events);
        for it in &det.items {
            println!("    {} {:>2}/{:<2}  {}", if it.earned { "✓" } else { "✗" }, if it.earned { it.points } else { 0 }, it.points, it.label);
        }
        let star = if det.cleared(&rubric) { "★ STAR EARNED" } else { "· no star" };
        println!(
            "  det {}/{}  = {}%   →  {}\n",
            det.earned, det.max, det.bps() / 100, star
        );
    }
}
